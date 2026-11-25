//! The engine API on one WebSocket. What the messages are: `wire::Request` and
//! `wire::Reply`. This file moves them, and holds four policies of its own:
//!
//! | Policy | Where |
//! |---|---|
//! | revision history: each edit keeps its moment until a snapshot passes it, paid in bytes. A press that changes the held lights keeps one that no bytes pay for (#312) | `Moment`, `REVISION_BYTES`, `Api::history` |
//! | an edit prepared on a base another change replaced is prepared again | `PREPARES` |
//! | an output edit waits for the old route to finish | `CUTOVER_WAIT` |
//! | health goes to a subscriber at most once per 500 ms of engine time | `HealthPacing` |
//!
//! | Route | What |
//! |---|---|
//! | `GET /health` | `ok` while the engine runs |
//! | `GET /version` | the API version, as text: a client checks it before it talks |
//! | `GET /api` | WebSocket: `Send`, `Query`, `Subscribe`, `Stop`, `Search`, `Fixture`, `Failures` in, JSON replies out |
//!
//! The plain routes allow any origin, so a web page can read them: they change nothing.
//! The WebSocket commands the lights, so only the pages in `ORIGINS` and in the
//! comma-separated `BOSSYLIGHTS_ORIGINS` (one more page per test run) may open it, and
//! clients that are no web page (they send no `Origin`). Any other page gets 403.

#![forbid(unsafe_code)]

mod health;
mod library;
mod report_line;
pub mod show;

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, ORIGIN};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use engine::{
    Begun, ChannelLevel, Command, Engine, Grouped, Looks, Patch, Patched, Project, ProjectView,
    Refusal, SavedLook, Sender, Snapshot,
};
use engine_core::{HeldLight, ShowFile, Text};
use library::Shelf;
use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc, watch};
use tokio::time::MissedTickBehavior;
use wire::{API_VERSION, Library, Reply, Request};

/// Serves the API on `listener` until a client sends `Stop`. Then every connection is
/// closed, the last change is saved, and this returns the engine: the caller reports and
/// drops it. The fixture library is read from `fixtures` while the engine runs. The patch
/// and the groups are opened from the show file `show` and saved to it after every change.
pub async fn serve(
    listener: TcpListener,
    mut engine: Engine,
    mut sender: Sender,
    fixtures: Option<PathBuf>,
    show: Option<PathBuf>,
) -> io::Result<Engine> {
    let (stop, mut stopped) = watch::channel(false);
    let shelf = library::start(fixtures);
    let opened = show
        .as_deref()
        .map_or(ShowFile::None, |path| show::open(path, &mut sender));
    let (kept, shown) = watch::channel(opened);
    let (moments, taken) = mpsc::unbounded_channel();
    let saved = show::Kept::of(&sender);
    let (patch, _) = watch::channel(saved.clone());
    let keeper = show.filter(|_| opened.saves()).map(|path| {
        tokio::spawn(show::keep(
            path,
            saved,
            patch.subscribe(),
            kept,
            stop.subscribe(),
        ))
    });
    let first = Moment::of(&sender);
    // A snapshot of a wake after every command queued so far: taken under `first`.
    let requested_at = Instant::now();
    let snapshot = engine.fresh().map_err(io::Error::other)?;
    let mut health = health::Reader::new();
    health.observe(|output, sample| snapshot.health_sample(output, sample));
    let published = health.publish();
    let picture = Picture::of(
        snapshot.clone(),
        requested_at,
        &first,
        &shelf,
        &shown,
        published,
    );
    let through = Arc::clone(&first.through);
    let grabs = sender.grabs_revision();
    let front = Front {
        shelf: shelf.clone(),
        moments: taken,
        kept: VecDeque::from([first]),
        show: shown,
    };
    let (painted, pictures) = watch::channel(Arc::new(picture));
    let reader = tokio::spawn(read(engine, front, health, painted, stop.subscribe()));
    let api = Arc::new(Api {
        moments,
        patch,
        sender: Mutex::new(Publisher {
            sender,
            through,
            grabs,
        }),
        history: Arc::new(Semaphore::new(engine::PLAN_BYTES)),
        stop,
        pictures,
        shelf,
        origins: std::env::var("BOSSYLIGHTS_ORIGINS")
            .map(|list| list.split(',').map(str::to_owned).collect())
            .unwrap_or_default(),
    });
    let any = || [(ACCESS_CONTROL_ALLOW_ORIGIN, "*")];
    let routes = Router::new()
        .route("/health", get(move || async move { (any(), "ok") }))
        .route(
            "/version",
            get(move || async move { (any(), API_VERSION.to_string()) }),
        )
        .route("/api", get(connect))
        .with_state(Arc::clone(&api));
    let served = axum::serve(listener, routes)
        .with_graceful_shutdown(async move {
            let _ = stopped.wait_for(|stop| *stop).await;
        })
        .await;
    api.stopping().await;
    if let Some(keeper) = keeper {
        keeper.await.map_err(io::Error::other)?;
    }
    let engine = reader.await.map_err(io::Error::other)?;
    served.map(|()| engine)
}

struct Api {
    /// Pages allowed besides `ORIGINS`, from the environment at start.
    origins: Vec<String>,
    /// The engine front; connections take turns. Nobody holds it while a device opens.
    /// The tick thread never sees this lock.
    sender: Mutex<Publisher>,
    /// Capacity for revisions awaiting a snapshot, including compiled plans they pin.
    history: Arc<Semaphore>,
    /// The front after each change, in the order of the changes: the reader pairs each
    /// snapshot with the one it was taken under.
    moments: mpsc::UnboundedSender<Moment>,
    /// The front's patch and looks after the last command, for the show file.
    patch: watch::Sender<show::Kept>,
    /// Turns true once, on `Stop`. Every connection and the server watch it.
    stop: watch::Sender<bool>,
    /// The newest frame and its output health, shared by every connection.
    pictures: watch::Receiver<Arc<Picture>>,
    /// The fixture library, once it is read.
    shelf: watch::Receiver<Arc<Shelf>>,
}

/// A frame as every connection shows it: the engine's snapshot, and the library, the
/// project, the patch and the show file as they were when the snapshot was taken.
struct Picture {
    snapshot: Snapshot,
    /// Host-clock lower bound on when the reader requested this snapshot.
    requested_at: Instant,
    health: health::View,
    library: Library,
    project: Project,
    patch: Arc<Patch>,
    looks: Arc<[SavedLook]>,
    /// Compare authored revisions for deltas without pinning their compiled plans.
    source: Weak<Looks>,
    show: ShowFile,
    /// What the grabs hold, in light order.
    held: Arc<[HeldLight]>,
}

impl Picture {
    fn of(
        snapshot: Snapshot,
        requested_at: Instant,
        moment: &Moment,
        shelf: &watch::Receiver<Arc<Shelf>>,
        show: &watch::Receiver<ShowFile>,
        health: health::View,
    ) -> Self {
        let project = moment.project.at(&snapshot);
        Self {
            snapshot,
            requested_at,
            health,
            library: shelf.borrow().library(),
            project,
            patch: Arc::clone(&moment.patch),
            looks: Arc::clone(moment.rows.get_or_init(|| moment.looks.all().into())),
            source: Arc::downgrade(&moment.looks),
            show: *show.borrow(),
            held: Arc::clone(&moment.held),
        }
    }
}

/// A revision covers every completed FIFO prefix from `sent` through `through`.
/// A performance command extends that range without retaining another project, unless
/// it changes what the grabs hold.
struct Moment {
    sent: u64,
    through: Arc<AtomicU64>,
    /// Pays for the preceding revision until the reader passes it.
    credit: Option<OwnedSemaphorePermit>,
    revision: u64,
    project: ProjectView,
    patch: Arc<Patch>,
    looks: Arc<Looks>,
    /// Materialize one plan-free copy for all pictures taken under this revision.
    rows: OnceLock<Arc<[SavedLook]>>,
    /// What the grabs hold, from the front's book.
    held: Arc<[HeldLight]>,
}

impl Moment {
    fn of(sender: &Sender) -> Self {
        Self {
            sent: sender.sent(),
            through: Arc::new(AtomicU64::new(sender.sent())),
            credit: None,
            revision: sender.revision(),
            project: sender.project_for_snapshot(),
            patch: sender.patch(),
            looks: sender.looks(),
            rows: OnceLock::new(),
            held: sender.held_lights().into(),
        }
    }
}

/// The moment a snapshot was taken under, if it is known: the one of the `revision` it
/// read at its cutoff, whose complete FIFO prefixes include `applied`. `None` while the tick is part
/// way through the commands of one change, or the change is not in `kept` yet. Forgets
/// the moments older than it: the revision never goes back.
fn under(kept: &mut VecDeque<Moment>, applied: u64, revision: u64) -> Option<&Moment> {
    while kept.get(1).is_some_and(|next| next.revision <= revision) {
        kept.pop_front();
    }
    let moment = kept.front_mut()?;
    if moment.revision != revision {
        return None;
    }
    // Every preceding revision has been dropped. Edits may use its bytes again.
    moment.credit = None;
    (moment.sent <= applied && applied <= moment.through.load(Ordering::Acquire)).then_some(moment)
}

/// What the front holds besides the engine, as the reader takes it into each picture.
struct Front {
    shelf: watch::Receiver<Arc<Shelf>>,
    /// The front's moments, as `send` makes them.
    moments: mpsc::UnboundedReceiver<Moment>,
    /// The moments a coming snapshot may be taken under, oldest first.
    kept: VecDeque<Moment>,
    show: watch::Receiver<ShowFile>,
}

impl Front {
    /// `snapshot` with the project and the patch it was taken under. `None` when that
    /// moment is not known yet.
    fn picture(
        &mut self,
        snapshot: &Snapshot,
        requested_at: Instant,
        health: health::View,
    ) -> Option<Picture> {
        while let Ok(moment) = self.moments.try_recv() {
            self.kept.push_back(moment);
        }
        let moment = under(&mut self.kept, snapshot.applied(), snapshot.revision())?;
        Some(Picture::of(
            snapshot.clone(),
            requested_at,
            moment,
            &self.shelf,
            &self.show,
            health,
        ))
    }
}

/// Poll twice a beat. Observe each frame once and publish the frame and health together
/// as soon as the matching project is available.
async fn read(
    mut engine: Engine,
    mut front: Front,
    mut health: health::Reader,
    painted: watch::Sender<Arc<Picture>>,
    mut stopped: watch::Receiver<bool>,
) -> Engine {
    let mut poll = tokio::time::interval(engine::BEAT / 2);
    poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut observed_frame = painted.borrow().snapshot.frame();
    let mut requested_at = painted.borrow().requested_at;
    let mut next_request = Instant::now();
    loop {
        tokio::select! {
            _ = stopped.wait_for(|stop| *stop) => return engine,
            _ = poll.tick() => {
                let poll_started = Instant::now();
                let snapshot = engine.latest();
                let frame = snapshot.frame();
                // The front may need another poll to find this frame's project.
                if frame != observed_frame {
                    // latest takes the previous request's snapshot and asks for the next.
                    requested_at = std::mem::replace(&mut next_request, poll_started);
                    health.observe(|output, sample| snapshot.health_sample(output, sample));
                    observed_frame = frame;
                }
                let health = health.publish();
                let changed = {
                    let last = painted.borrow();
                    frame != last.snapshot.frame()
                        || health.revision() != last.health.revision()
                };
                if changed && let Some(picture) = front.picture(snapshot, requested_at, health) {
                    painted.send_replace(Arc::new(picture));
                }
            }
        }
    }
}

/// The web pages that may open the API, and why. A browser always sends the page's
/// origin; the shell, tests and scripts send none.
/// The end-to-end tests add their page by `BOSSYLIGHTS_ORIGINS` (`e2e/ports.ts`).
const ORIGINS: [&str; 3] = [
    "tauri://localhost",      // the app window on macOS and Linux
    "http://tauri.localhost", // the app window on Windows
    "http://127.0.0.1:5174",  // the UI in a browser, `npm run ui`
];

async fn connect(
    headers: HeaderMap,
    socket: WebSocketUpgrade,
    State(api): State<Arc<Api>>,
) -> Response {
    match headers.get(ORIGIN) {
        Some(origin)
            if !ORIGINS.iter().any(|allowed| origin == allowed)
                && !api.origins.iter().any(|allowed| origin == allowed.as_str()) =>
        {
            StatusCode::FORBIDDEN.into_response()
        }
        _ => socket.on_upgrade(move |socket| talk(socket, api)),
    }
}

/// How many requests of one connection may wait for their replies. Past it the
/// connection is read no further until a reply goes out.
const WAITING: usize = 32;

/// A request of one connection whose reply has not gone out. Replies go out in the
/// order of their requests.
#[expect(
    clippy::large_enum_variant,
    reason = "a search or a fixture keeps its texts in place: at most `WAITING` slots a connection"
)]
enum Slot {
    Ready(Option<String>),
    /// A `Send` on the connection's worker. Its reply comes in order on the worker's
    /// results.
    Working,
    /// A request answered when its turn comes, from the newest picture or the library.
    Later(Later),
}

#[derive(Clone, Copy)]
enum Later {
    Query,
    Subscribe,
    Search { text: Text },
    Fixture { maker: Text, model: Text },
    Failures,
}

/// One connection: answers each request in order; once subscribed, sends what changed
/// with every new snapshot. Never waits on the tick thread, the tick thread never waits
/// on a client. A pending edit or device open never stops it reading: only edits go to
/// the connection's worker, one after another; a press runs at once (the socket table in
/// docs/engine.md, API).
async fn talk(mut socket: WebSocket, api: Arc<Api>) {
    let mut pictures = api.pictures.clone();
    // What a subscribed connection sent last.
    let mut subscription: Option<Subscription> = None;
    let mut stopped = api.stop.subscribe();
    let (jobs, mut results) = worker(Arc::clone(&api));
    let mut waiting: VecDeque<Slot> = VecDeque::with_capacity(WAITING);
    loop {
        while let Some(slot) = waiting.pop_front() {
            let reply = match slot {
                Slot::Ready(reply) => reply,
                Slot::Later(later) => answer(&api, later, &mut pictures, &mut subscription),
                Slot::Working => {
                    waiting.push_front(Slot::Working);
                    break;
                }
            };
            if !deliver(&mut socket, reply, &mut subscription).await {
                return; // client gone
            }
        }
        let health = subscription.as_mut().and_then(|sub| {
            let now = Arc::clone(&pictures.borrow());
            sub.health(&now)
        });
        if health.is_some() {
            if !deliver(&mut socket, health, &mut subscription).await {
                return;
            }
            continue;
        }
        tokio::select! {
            () = async { let _ = stopped.wait_for(|stop| *stop).await; } => {
                // Every request read gets its reply, then the connection closes.
                while let Some(slot) = waiting.pop_front() {
                    let reply = match slot {
                        Slot::Ready(reply) => reply,
                        Slot::Later(later) => answer(&api, later, &mut pictures, &mut subscription),
                        Slot::Working => results.recv().await.flatten(),
                    };
                    if !deliver(&mut socket, reply, &mut subscription).await {
                        return;
                    }
                }
                let _ = socket.send(Message::Close(None)).await;
                return;
            }
            message = socket.recv(), if waiting.len() < WAITING => match message {
                Some(Ok(Message::Text(text))) => {
                    let slot = take(&api, &text, &jobs).await;
                    waiting.push_back(slot);
                }
                Some(Ok(Message::Binary(_))) => {
                    waiting.push_back(Slot::Ready(json(&Reply::Refused { reason: Refusal::NotACommand })));
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                Some(Ok(Message::Close(_)) | Err(_)) | None => return,
            },
            Some(reply) = results.recv() => {
                if let Some(slot) = waiting.iter_mut().find(|slot| matches!(slot, Slot::Working)) {
                    *slot = Slot::Ready(reply);
                }
            }
            Ok(()) = pictures.changed(), if subscription.is_some() => {
                let now = Arc::clone(&pictures.borrow_and_update());
                let changed = subscription.as_mut().and_then(|sub| sub.changed(now));
                if !deliver(&mut socket, changed, &mut subscription).await {
                    return;
                }
            }
        }
    }
}

/// Sends `reply`, if any. `false` once the client is gone.
async fn deliver(
    socket: &mut WebSocket,
    reply: Option<String>,
    subscription: &mut Option<Subscription>,
) -> bool {
    let Some(reply) = reply else {
        return true;
    };
    if socket.send(Message::text(reply)).await.is_err() {
        return false;
    }
    if let Some(sub) = subscription {
        sub.sent();
    }
    true
}

/// The slot of one request of a connection. A press is admitted here, at once, even
/// behind a pending edit of this connection; every edit goes to the worker, behind what
/// is there. Replies keep the order of the requests. `Stop` stops the engine at once.
async fn take(api: &Arc<Api>, text: &str, jobs: &mpsc::UnboundedSender<Command>) -> Slot {
    let Ok(request) = serde_json::from_str::<Request>(text) else {
        return Slot::Ready(json(&Reply::Refused {
            reason: Refusal::NotACommand,
        }));
    };
    match request {
        Request::Send { command } if !is_edit(&command) => {
            Slot::Ready(json(&replied(send(api, command).await)))
        }
        Request::Send { command } => match jobs.send(command) {
            Ok(()) => Slot::Working,
            Err(_) => Slot::Ready(json(&Reply::Stopping)), // the worker ends only as the server stops
        },
        Request::Stop => {
            api.stopping().await;
            Slot::Ready(json(&Reply::Stopping))
        }
        Request::Query => Slot::Later(Later::Query),
        Request::Subscribe => Slot::Later(Later::Subscribe),
        Request::Search { text } => Slot::Later(Later::Search { text }),
        Request::Fixture { maker, model } => Slot::Later(Later::Fixture { maker, model }),
        Request::Failures => Slot::Later(Later::Failures),
    }
}

/// The connection's worker: sends its commands one after another, in the order they
/// came, and gives each reply back in that order. A command not started when the
/// connection closes is dropped.
fn worker(
    api: Arc<Api>,
) -> (
    mpsc::UnboundedSender<Command>,
    mpsc::UnboundedReceiver<Option<String>>,
) {
    let (jobs, mut todo) = mpsc::unbounded_channel::<Command>();
    let (done, results) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(command) = todo.recv().await {
            if done.is_closed() {
                return;
            }
            // Its own task: a panic in it answers `Failed`, and the next command runs.
            let job = tokio::spawn({
                let api = Arc::clone(&api);
                async move { send(&api, command).await }
            });
            let reply = match job.await {
                Ok(sent) => replied(sent),
                Err(_) => failed(&api, "a command"),
            };
            let _ = done.send(json(&reply));
        }
    });
    (jobs, results)
}

fn replied(sent: Result<Reply<'static>, Refusal>) -> Reply<'static> {
    sent.unwrap_or_else(|reason| Reply::Refused { reason })
}

/// The reply to a request answered when its turn comes.
fn answer(
    api: &Api,
    later: Later,
    pictures: &mut watch::Receiver<Arc<Picture>>,
    subscription: &mut Option<Subscription>,
) -> Option<String> {
    match later {
        Later::Query => {
            let now = Arc::clone(&pictures.borrow());
            picture(&now)
        }
        // The subscription starts as its picture goes out: no change comes before it.
        Later::Subscribe => {
            let now = Arc::clone(&pictures.borrow_and_update());
            let reply = picture(&now);
            *subscription = Some(Subscription::new(now));
            reply
        }
        Later::Search { text } => api.shelf.borrow().found(text.as_str()),
        Later::Fixture { maker, model } => {
            api.shelf.borrow().fixture(maker.as_str(), model.as_str())
        }
        Later::Failures => api.shelf.borrow().failures(),
    }
}

/// An edit is prepared off the front and may wait for a device: an engine edit
/// ([`engine::edits`]) or an output change.
fn is_edit(command: &Command) -> bool {
    matches!(
        command,
        Command::SetOutput { .. } | Command::RemoveOutput { .. }
    ) || engine::edits(command)
}

/// The only writer of the current revision's FIFO range.
struct Publisher {
    sender: Sender,
    through: Arc<AtomicU64>,
    /// The grabs' revision ([`Sender::grabs_revision`]) the current moment holds.
    grabs: u64,
}

/// How often a command prepared on a base another change replaced is prepared again,
/// before the client is told `Stale`.
const PREPARES: usize = 3;

/// An output writer may wait for its previous route to finish. Other writers
/// keep using the front, including a performer sending a press to either run.
const CUTOVER_WAIT: Duration = Duration::from_secs(10);

/// Under `test-support`, removing the group of this name panics in its prepare.
#[cfg(feature = "test-support")]
pub const PANICS: &str = "panics in prepare";

/// Under `test-support`, the names of the looks whose edits were handed to the blocking
/// pool, in this process.
#[cfg(feature = "test-support")]
static HANDED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

#[cfg(feature = "test-support")]
fn handed(command: &Command) {
    if let Command::PutStep { name, .. }
    | Command::PutPattern { name, .. }
    | Command::PutSequence { name, .. } = command
        && let Ok(mut names) = HANDED.lock()
    {
        names.push(name.as_str().to_owned());
    }
}

/// Under `test-support`: whether an edit of the look named `name` was handed to the
/// blocking pool. A test waits on it instead of on time.
#[cfg(feature = "test-support")]
pub fn handed_to_the_pool(name: &str) -> bool {
    HANDED
        .lock()
        .is_ok_and(|names| names.iter().any(|handed| handed == name))
}

/// Even an edit which shares all its plans needs a place in snapshot history.
const REVISION_BYTES: usize = engine::PLAN_BYTES.div_ceil(engine::QUEUE);

fn reserve(history: &Arc<Semaphore>, bytes: usize) -> Result<OwnedSemaphorePermit, Refusal> {
    Arc::clone(history)
        .try_acquire_many_owned(u32::try_from(bytes).map_err(|_| Refusal::WorkLimit)?)
        .map_err(|_| Refusal::WorkLimit)
}

/// Sends `command` to the front, with the library as it is read so far. A press is
/// prepared and admitted under the front at once: no thread hop between it and the next
/// frame. An edit ([`engine::edits`]) is prepared on a thread of the blocking pool,
/// without the front (a patch measures its show file, a look compiles), and admitted
/// under it: every other client is answered meanwhile. So is a device it needs opened.
/// The answer when it is taken, `Stopping` when the engine stops first: nothing changed.
async fn send(api: &Arc<Api>, command: Command) -> Result<Reply<'static>, Refusal> {
    let shelf = Arc::clone(&api.shelf.borrow());
    let mut credit = None;
    let begun = if is_edit(&command) {
        edit(api, command, &shelf, &mut credit).await
    } else {
        api.changing(&mut credit, |sender| {
            sender.begin(command, shelf.fixtures())
        })
        .await
        .map_or(Edited::Stopped, Edited::from_result)
    };
    match begun {
        Edited::Stopped => Ok(Reply::Stopping),
        Edited::Failed => Ok(failed(api, "an edit")),
        Edited::Begun(begun) => match *begun {
            Begun::Opening(opening) => {
                let opener = Arc::clone(api);
                let mut stopped = api.stop.subscribe();
                // Keep the opener through the handoff. A busy blocking pool cannot
                // put another job between publication and delivery of the device.
                let worker = tokio::task::spawn_blocking(move || {
                    let opened = opening.open();
                    let publish =
                        opener.changing(&mut credit, |sender| sender.publish_output(opened));
                    let Some(result) = tokio::runtime::Handle::current().block_on(publish) else {
                        return Ok(Reply::Stopping);
                    };
                    if let Some(handoff) = result?
                        && !handoff.wait()
                    {
                        return Ok(Reply::Stopping);
                    }
                    Ok(Reply::Accepted)
                });
                // T13 answers the writer while the opener still owns and drains
                // the old device. The worker finishes cleanup when the driver returns.
                tokio::select! {
                    biased;
                    _ = stopped.wait_for(|stop| *stop) => Ok(Reply::Stopping),
                    answer = worker => answer.unwrap_or_else(|_| Ok(failed(api, "a device open"))),
                }
            }
            Begun::Queued => Ok(Reply::Accepted),
            Begun::Patched { fixture } => Ok(Reply::Patched { fixture }),
            Begun::Started { play } => Ok(Reply::Started { play }),
            Begun::Grabbed { grab } => Ok(Reply::Grabbed { grab }),
        },
        Edited::Refused(reason) => Err(reason),
    }
}

/// What sending a command came to, before its reply.
enum Edited {
    Begun(Box<Begun>),
    Refused(Refusal),
    /// The engine stopped first: nothing changed.
    Stopped,
    /// The work panicked. The picture says what is live.
    Failed,
}

impl Edited {
    fn from_result(result: Result<Begun, Refusal>) -> Self {
        match result {
            Ok(begun) => Self::Begun(Box::new(begun)),
            Err(reason) => Self::Refused(reason),
        }
    }
}

/// `Failed`, told on the process's error output, or `Stopping` once the engine stops:
/// then a cancelled task is no fault.
fn failed(api: &Api, work: &str) -> Reply<'static> {
    if *api.stop.borrow() {
        return Reply::Stopping;
    }
    eprintln!("{}", report_line::failed(work));
    Reply::Failed
}

/// Prepares the edit `command` on the blocking pool and admits it, again while another
/// change replaced its base, [`PREPARES`] times at most.
async fn edit(
    api: &Api,
    command: Command,
    shelf: &Arc<Shelf>,
    opening_credit: &mut Option<OwnedSemaphorePermit>,
) -> Edited {
    let mut begun = Edited::Refused(Refusal::Stale);
    let waiting_since = Instant::now();
    for _ in 0..PREPARES {
        // T12. Read the base and the state of this output under the same lock.
        // Refusal and stop can both end the wait while the engine clock is held.
        let base = loop {
            let front = api.sender.lock().await;
            if *api.stop.borrow() {
                return Edited::Stopped;
            }
            if !matches!(
                command,
                Command::SetOutput { output, .. } | Command::RemoveOutput { output }
                    if front.sender.cutover_pending(output)
            ) {
                break front.sender.base();
            }
            drop(front);
            if waiting_since.elapsed() >= CUTOVER_WAIT {
                return Edited::Refused(Refusal::Opening);
            }
            tokio::time::sleep(engine::BEAT).await;
        };
        // Bound in-flight edits too. A full history refuses before compiling.
        let mut credit = match reserve(&api.history, REVISION_BYTES) {
            Ok(credit) => credit,
            Err(reason) => return Edited::Refused(reason),
        };
        let shelf = Arc::clone(shelf);
        let history = Arc::clone(&api.history);
        #[cfg(feature = "test-support")]
        handed(&command);
        let prepared = tokio::task::spawn_blocking(move || {
            #[cfg(feature = "test-support")]
            if matches!(command, Command::RemoveGroup { name } if name.as_str() == PANICS) {
                panic!("a prepare that panics, for a test");
            }
            let prepared = base.prepare(command, shelf.fixtures())?;
            let extra = prepared
                .replaced_plan_bytes()
                .saturating_sub(credit.num_permits());
            if extra > 0 {
                credit.merge(reserve(&history, extra)?);
            }
            Ok((prepared, credit))
        })
        .await;
        begun = match prepared {
            Ok(Ok((prepared, credit))) => {
                let mut credit = Some(credit);
                let admitted = api
                    .changing(&mut credit, |sender| sender.admit(prepared))
                    .await;
                if matches!(admitted, Some(Ok(Begun::Opening(_)))) {
                    // The pending open has no visible change yet. Keep its place
                    // in history until the device is ready and finish publishes it.
                    *opening_credit = credit;
                }
                admitted.map_or(Edited::Stopped, Edited::from_result)
            }
            // refused before the engine stops, or answered `Stopping` as an admitted one is
            Ok(Err(reason)) if !*api.stop.borrow() => Edited::Refused(reason),
            Ok(Err(_)) => Edited::Stopped,
            Err(_) if *api.stop.borrow() => Edited::Stopped, // cancelled: the engine stops
            Err(_) => Edited::Failed,
        };
        if !matches!(begun, Edited::Refused(Refusal::Stale)) {
            break;
        }
    }
    begun
}

impl Api {
    /// Changes the front with `change`, and tells the reader the front as `change` left
    /// it, before another change: each moment is one the engine was in. A press that
    /// changes what the grabs hold is a moment of its own, without credit: it keeps its
    /// held lights (up to one per light), paid by no bytes, until a snapshot passes it.
    /// Bounded while the reader keeps up, not while it stalls (#312). The show file
    /// hears only of a change of the patch or the looks: a refusal or a press saves
    /// nothing. `None` once the engine stops ([`Api::stopping`]): nothing changes then,
    /// so no change comes after the last save.
    async fn changing<T>(
        &self,
        credit: &mut Option<OwnedSemaphorePermit>,
        change: impl FnOnce(&mut Sender) -> T,
    ) -> Option<T> {
        let mut publisher = self.sender.lock().await;
        if *self.stop.borrow() {
            return None;
        }
        let publisher = &mut *publisher;
        let sender = &mut publisher.sender;
        let before = sender.project_for_snapshot();
        let changed = change(sender);
        let edited = self.patch.send_if_modified(|kept| kept.follow(sender));
        let outputs = !before.same_outputs(&sender.project_for_snapshot());
        let holds = sender.grabs_revision() != publisher.grabs;
        if edited || outputs || holds {
            sender.revise();
            let mut moment = Moment::of(sender);
            moment.credit = credit.take();
            debug_assert!(moment.credit.is_some() || !(edited || outputs));
            publisher.through = Arc::clone(&moment.through);
            publisher.grabs = sender.grabs_revision();
            // The reader is gone only once the server stops.
            let _ = self.moments.send(moment);
        } else {
            let sent = sender.sent();
            publisher.through.store(sent, Ordering::Release);
        }
        Some(changed)
    }

    /// Stops the engine: under the front, so every change admitted before is one the
    /// last save has, and none is admitted after.
    async fn stopping(&self) {
        let front = self.sender.lock().await;
        front.sender.stop_outputs();
        self.stop.send_replace(true);
    }
}

fn picture(now: &Picture) -> Option<String> {
    json(&Reply::Picture {
        state: now.snapshot.performance(),
        health: now.health.health(),
        project: now.project,
        library: now.library,
        patch: &now.patch.fixtures(),
        groups: &now.patch.groups(),
        looks: &now.looks,
        show: now.show,
        universes: now.snapshot.universes(),
        lights: &now.held,
    })
}

/// Each subscriber tracks frame deltas and health deliveries independently.
struct Subscription {
    last: Arc<Picture>,
    revision: u64,
    cursor: u64,
    pacing: HealthPacing,
}

/// When a subscriber gets health (docs/engine.md, "at most once per 500 ms"). A
/// message may finish sending long after the wake it pictures; a snapshot requested
/// after the send gives an upper bound on the engine time of the send. The host clock
/// only orders sends and snapshot requests.
///
/// | Pacing \ event | a message went out | a picture requested after that send | a picture: health changed, 500 ms of engine time since |
/// |---|---|---|---|
/// | `Sending` | `AwaitingSample(send time)` | same | same |
/// | `AwaitingSample` | same | `Since(that picture's engine time)` | same |
/// | `Since` | same | same | send health, `Sending` |
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HealthPacing {
    Sending,
    AwaitingSample(Instant),
    Since(Duration),
}

#[derive(Clone, Copy, Debug)]
enum PacingEvent {
    /// Any message went out to the subscriber at `at` on the host clock.
    Sent { at: Instant },
    /// A new picture: when its snapshot was requested, its engine time, and whether its
    /// health differs from the last health this subscriber got.
    Picture {
        requested_at: Instant,
        now: Duration,
        changed: Changed,
    },
}

/// Whether a picture's health differs from the last one a subscriber got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Changed {
    Yes,
    No,
}

/// Send the health now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SendHealth;

impl HealthPacing {
    /// The table above, as one calculation.
    fn next(self, event: PacingEvent) -> (Self, Option<SendHealth>) {
        const MOST_OFTEN: Duration = Duration::from_millis(500);
        match (self, event) {
            (Self::Sending, PacingEvent::Sent { at }) => (Self::AwaitingSample(at), None),
            (
                Self::AwaitingSample(sent),
                PacingEvent::Picture {
                    requested_at, now, ..
                },
            ) if requested_at > sent => (Self::Since(now), None),
            (
                Self::Since(sent),
                PacingEvent::Picture {
                    now,
                    changed: Changed::Yes,
                    ..
                },
            ) if now.saturating_sub(sent) >= MOST_OFTEN => (Self::Sending, Some(SendHealth)),
            (Self::AwaitingSample(_) | Self::Since(_), PacingEvent::Sent { .. })
            | (
                Self::Sending | Self::AwaitingSample(_) | Self::Since(_),
                PacingEvent::Picture { .. },
            ) => (self, None),
        }
    }
}

impl Subscription {
    fn new(picture: Arc<Picture>) -> Self {
        Self {
            revision: picture.health.revision(),
            cursor: picture.health.end(),
            last: picture,
            pacing: HealthPacing::Sending,
        }
    }

    fn sent(&mut self) {
        self.pacing = self.pacing.next(PacingEvent::Sent { at: Instant::now() }).0;
    }

    fn health(&mut self, picture: &Picture) -> Option<String> {
        let view = &picture.health;
        let changed = if self.revision == view.revision() {
            Changed::No
        } else {
            Changed::Yes
        };
        let (pacing, send) = self.pacing.next(PacingEvent::Picture {
            requested_at: picture.requested_at,
            now: picture.snapshot.now(),
            changed,
        });
        self.pacing = pacing;
        send?;
        let (events, missed) = view.events_since(self.cursor);
        self.cursor = view.end();
        self.revision = view.revision();
        json(&Reply::Health {
            health: view.health(),
            events: &events,
            missed,
        })
    }

    fn changed(&mut self, now: Arc<Picture>) -> Option<String> {
        let last = std::mem::replace(&mut self.last, now);
        if last.snapshot.frame() == self.last.snapshot.frame() {
            return None;
        }
        changed(&last, &self.last)
    }
}

/// The channels and lights that differ from this connection's last frame.
fn changed(last: &Picture, now: &Picture) -> Option<String> {
    let channels: Vec<ChannelLevel> = now.snapshot.channel_changes_since(&last.snapshot).collect();
    let lights = held_changes(&last.held, &now.held);
    let new = !Arc::ptr_eq(&last.patch, &now.patch);
    let fixtures: Option<Vec<Patched>> = new.then(|| now.patch.fixtures());
    let groups: Option<Vec<Grouped>> = new.then(|| now.patch.groups());
    let looks = (!Weak::ptr_eq(&last.source, &now.source)).then_some(now.looks.as_ref());
    json(&Reply::Changed {
        state: now.snapshot.performance(),
        project: now.project,
        library: now.library,
        patch: fixtures.as_deref(),
        groups: groups.as_deref(),
        looks,
        show: now.show,
        channels: &channels,
        lights: &lights,
    })
}

/// The lights whose holds differ between `last` and `now`, both in light order: a light
/// no longer held as one holding nothing.
fn held_changes(last: &[HeldLight], now: &[HeldLight]) -> Vec<HeldLight> {
    let free = |light| HeldLight {
        light,
        intensity: None,
        colour: None,
    };
    let (mut last, mut now) = (last.iter().peekable(), now.iter().peekable());
    let mut changes = Vec::new();
    loop {
        let Some(light) = [last.peek(), now.peek()]
            .into_iter()
            .flatten()
            .map(|held| held.light)
            .min()
        else {
            return changes;
        };
        let was = last.next_if(|held| held.light == light);
        let is = now.next_if(|held| held.light == light);
        if was != is {
            changes.push(is.copied().unwrap_or(free(light)));
        }
    }
}

/// A reply is plain numbers and names: this does not fail.
fn json(reply: &Reply) -> Option<String> {
    serde_json::to_string(reply).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::{ChannelNumbers, Light, Rgb, Target, Text, Universes};
    use engine_core::Step;
    use serde_json::Value;
    use std::io;

    /// The test definition of a Generic RGB, read when the test is built.
    const GENERIC_RGB: &[u8] =
        include_bytes!("../../fixture-library/tests/definitions/Generic-Generic-RGB.qxf");

    /// Look `look` saved as fixture 1 in `colour`.
    fn step(look: u16, colour: Rgb) -> Command {
        Command::PutStep {
            look,
            name: Text::new("step").unwrap(),
            step: Step {
                target: Target::Fixture { fixture: 1 },
                values: Light {
                    colour: Some(colour),
                    ..Light::DARK
                },
            }
            .into(),
        }
    }

    #[test]
    fn a_picture_joins_the_looks_and_holds_of_its_own_snapshot_when_revision_b_overtakes_it() {
        let red = Rgb {
            red: u16::MAX,
            green: 0,
            blue: 0,
        };
        let green = Rgb {
            red: 0,
            green: u16::MAX,
            blue: 0,
        };
        let blue = Rgb {
            red: 0,
            green: 0,
            blue: u16::MAX,
        };
        let library = [fixture_library::parse("Generic-Generic-RGB.qxf", GENERIC_RGB).unwrap()];
        let (mut engine, mut sender) = Engine::start(Universes::new(1).unwrap()).unwrap();
        let (moments, taken) = mpsc::unbounded_channel();
        let mut front = Front {
            shelf: library::start(None),
            moments: taken,
            kept: VecDeque::from([Moment::of(&sender)]),
            show: watch::channel(ShowFile::None).1,
        };
        // as `changing` does: each change, then its moment
        let mut change = |command| {
            let begun = sender.begin(command, &library).unwrap();
            sender.revise();
            moments.send(Moment::of(&sender)).unwrap();
            begun
        };
        let patch = Command::Patch {
            name: Text::new("PAR").unwrap(),
            maker: Text::new("Generic").unwrap(),
            model: Text::new("Generic RGB").unwrap(),
            mode: Text::new("RGB").unwrap(),
            at: ChannelNumbers {
                universe: 1,
                address: 1,
            },
        };
        change(patch);
        change(step(1, red));
        let Begun::Started { play } = change(Command::StartLook {
            look: 1,
            rank: engine::Rank::Manual,
            when: engine::When::Now,
            usage: Default::default(),
        }) else {
            panic!("look 1 plays");
        };
        // revision A: look 1 green, its play takes the new plan. A's snapshot is taken
        change(step(1, green));
        let a = engine.fresh().unwrap().clone();
        // revision B overtakes it before it is painted: the play released, look 1
        // removed, look 2 saved
        change(Command::ReleaseHold { play });
        change(Command::RemoveLook { look: 1 });
        change(step(2, blue));
        let health = health::View::default();
        let a = front
            .picture(&a, Instant::now(), health.clone())
            .expect("A's moment is kept");
        let b = engine.fresh().unwrap().clone();
        let b = front
            .picture(&b, Instant::now(), health)
            .expect("B's moment is kept");

        // (picture, saved looks, the play's look, light 1's bytes)
        let cases = [
            (&a, vec![1], Some(1), [0, 255, 0]),
            (&b, vec![2], None, [0, 0, 0]),
        ];
        for (picture, looks, playing, bytes) in cases {
            let saved: Vec<u16> = picture.looks.iter().map(|saved| saved.look).collect();
            assert_eq!(saved, looks);
            let plays: Vec<u16> = picture
                .snapshot
                .active_plays()
                .map(|play| play.look)
                .collect();
            assert_eq!(plays.first().copied(), playing);
            assert_eq!([1, 2, 3].map(|at| picture.snapshot.sent(1, at)), bytes);
        }
        // Query: each picture as it is
        let query: Value = serde_json::from_str(&picture(&a).unwrap()).unwrap();
        assert_eq!(query["Picture"]["looks"][0]["look"], 1);
        assert_eq!(query["Picture"]["state"]["plays"][play.slot()]["look"], 1);
        // Subscribe: A delivered, then B's change carries B's looks
        let mut subscription = Subscription::new(Arc::new(a));
        let changed: Value =
            serde_json::from_str(&subscription.changed(Arc::new(b)).unwrap()).unwrap();
        let looks = &changed["Changed"]["looks"];
        assert_eq!(
            (looks[0]["look"].clone(), looks[1].clone()),
            (2.into(), Value::Null)
        );
        assert!(changed["Changed"]["state"]["plays"][play.slot()].is_null());
    }

    #[test]
    fn a_slow_subscriber_keeps_the_authored_row_without_pinning_the_compiled_plan() {
        let (mut engine, mut sender) = Engine::start(Universes::new(1).unwrap()).unwrap();
        sender
            .send(Command::PatchStrip {
                name: Text::new("pixel").unwrap(),
                pixels: engine_core::Pixels::try_from(1).unwrap(),
                color: engine::Color::Rgb,
                at: ChannelNumbers {
                    universe: 1,
                    address: 1,
                },
            })
            .unwrap();
        sender.send(step(1, Rgb::BLACK)).unwrap();
        let plan = Arc::downgrade(&sender.looks().plan(1).unwrap());
        let delivered = Picture::of(
            engine.fresh().unwrap().clone(),
            Instant::now(),
            &Moment::of(&sender),
            &library::start(None),
            &watch::channel(ShowFile::None).1,
            health::View::default(),
        );
        sender.send(Command::RemoveLook { look: 1 }).unwrap();
        assert!(
            plan.upgrade().is_none(),
            "readback still owns the compiled plan"
        );
        let reply: Value = serde_json::from_str(&picture(&delivered).unwrap()).unwrap();
        assert_eq!(reply["Picture"]["looks"][0]["look"], 1);
    }

    #[test]
    fn a_picture_shows_the_looks_renamed_before_its_cutoff_never_after() {
        let library = [fixture_library::parse("Generic-Generic-RGB.qxf", GENERIC_RGB).unwrap()];
        let red = Rgb {
            red: u16::MAX,
            green: 0,
            blue: 0,
        };
        let named = |name| Command::PutStep {
            look: 1,
            name: Text::new(name).unwrap(),
            step: Step {
                target: Target::Fixture { fixture: 1 },
                values: Light {
                    colour: Some(red),
                    ..Light::DARK
                },
            }
            .into(),
        };
        // (when idle look 1 is renamed from A to B, the name in the picture of that wake)
        let cases = [("before the wake", "B"), ("at its held cutoff", "A")];
        for (when, name) in cases {
            let none: engine::Open = Box::new(|_| Err(io::Error::other("no devices")));
            let (mut engine, mut sender, wakes) =
                Engine::start_stepped(Universes::new(1).unwrap(), none).unwrap();
            let (moments, taken) = mpsc::unbounded_channel();
            let mut front = Front {
                shelf: library::start(None),
                moments: taken,
                kept: VecDeque::from([Moment::of(&sender)]),
                show: watch::channel(ShowFile::None).1,
            };
            // as `changing` does: each change, then its moment
            let mut change = |command| {
                sender.begin(command, &library).unwrap();
                sender.revise();
                moments.send(Moment::of(&sender)).unwrap();
            };
            change(Command::Patch {
                name: Text::new("PAR").unwrap(),
                maker: Text::new("Generic").unwrap(),
                model: Text::new("Generic RGB").unwrap(),
                mode: Text::new("RGB").unwrap(),
                at: ChannelNumbers {
                    universe: 1,
                    address: 1,
                },
            });
            change(named("A"));
            wakes.hold().unwrap();
            wakes.wake().unwrap(); // the patch applied
            let _ = engine.latest(); // asks for the next wake's picture
            if when == "before the wake" {
                change(named("B"));
            }
            wakes.cut().unwrap();
            if when == "at its held cutoff" {
                change(named("B"));
            }
            wakes.finish().unwrap();
            let snapshot = engine.latest().clone();
            let picture = front
                .picture(&snapshot, Instant::now(), health::View::default())
                .expect("its moment is kept");
            let shown = picture
                .looks
                .iter()
                .find(|saved| saved.look == 1)
                .map(|saved| saved.name.as_str());
            assert_eq!(shown, Some(name), "renamed {when}");
            wakes.run();
        }
    }

    #[test]
    fn a_snapshot_is_paired_only_with_the_moment_of_its_revision_at_exactly_its_applied_commands() {
        let (_engine, sender) = Engine::start(Universes::new(1).unwrap()).unwrap();
        // (sent of each kept moment, its revision its place; applied and the revision read
        // at the cutoff; the revision found; the revisions kept after)
        let cases = [
            (vec![0], 0, 0, Some(0), vec![0]),
            (vec![0], 1, 0, None, vec![0]), // queued, its moment not here yet
            (vec![0], 0, 1, None, vec![0]), // ended, its moment not here yet
            (vec![0, 2], 1, 1, None, vec![1]), // part way through one change
            (vec![0, 2], 2, 1, Some(1), vec![1]),
            (vec![0, 2, 2], 2, 1, Some(1), vec![1, 2]), // a rename after the cutoff
            (vec![0, 2, 2], 2, 2, Some(2), vec![2]),    // a rename before it
            (vec![0, 2, 5], 3, 2, None, vec![2]),
            (vec![0, 2, 5], 5, 2, Some(2), vec![2]),
        ];
        for (sents, applied, revision, found, left) in cases {
            // each revision its own looks: what a picture of it shows
            let looks: Vec<Arc<Looks>> = sents.iter().map(|_| Arc::new(Looks::new())).collect();
            let mut kept: VecDeque<Moment> = (0..)
                .zip(&sents)
                .zip(&looks)
                .map(|((revision, &sent), looks)| Moment {
                    sent,
                    revision,
                    through: Arc::new(AtomicU64::new(sent)),
                    looks: Arc::clone(looks),
                    ..Moment::of(&sender)
                })
                .collect();
            let case = format!("{sents:?} at {applied}, revision {revision}");
            let moment = under(&mut kept, applied, revision).map(|moment| {
                let at = usize::try_from(moment.revision).unwrap();
                assert!(Arc::ptr_eq(&moment.looks, &looks[at]), "{case}");
                moment.revision
            });
            assert_eq!(moment, found, "{case}");
            let after: Vec<u64> = kept.iter().map(|moment| moment.revision).collect();
            assert_eq!(after, left, "{case}");
        }
    }

    /// Each way a light goes between two pictures: newly held, held the same, held
    /// otherwise, no longer held, and lights on either side. Expected answers: the wire
    /// test `held_lights.rs` (a released light is carried holding nothing).
    #[test]
    fn a_change_carries_each_light_whose_holds_differ() {
        let held = |light, intensity| HeldLight {
            light,
            intensity,
            colour: None,
        };
        // (last, now, carried)
        let cases = [
            (vec![], vec![], vec![]),
            (vec![], vec![held(2, Some(1))], vec![held(2, Some(1))]),
            (vec![held(2, Some(1))], vec![held(2, Some(1))], vec![]),
            (
                vec![held(2, Some(1))],
                vec![held(2, Some(9))],
                vec![held(2, Some(9))],
            ),
            (vec![held(2, Some(1))], vec![], vec![held(2, None)]),
            (
                vec![held(1, Some(1)), held(3, Some(3))],
                vec![held(2, Some(2)), held(3, Some(3))],
                vec![held(1, None), held(2, Some(2))],
            ),
            (
                vec![held(2, Some(2))],
                vec![held(1, Some(1)), held(3, Some(3))],
                vec![held(1, Some(1)), held(2, None), held(3, Some(3))],
            ),
        ];
        for (last, now, carried) in cases {
            assert_eq!(held_changes(&last, &now), carried, "{last:?} to {now:?}");
        }
    }

    /// Every cell of the `HealthPacing` table. Expected answers: docs/engine.md (health
    /// at most once per 500 ms of engine time) and the send rule written above the table.
    #[test]
    fn health_pacing_follows_its_table() {
        let start = Instant::now();
        let sent = start + Duration::from_millis(10);
        let before = start;
        let after = start + Duration::from_millis(20);
        let at = |ms| Duration::from_millis(ms);
        let picture = |requested_at, now, changed| PacingEvent::Picture {
            requested_at,
            now,
            changed,
        };
        let send = PacingEvent::Sent { at: sent };
        let since = HealthPacing::Since(at(1_000));
        let awaiting = HealthPacing::AwaitingSample(sent);
        let cases = [
            (HealthPacing::Sending, send, (awaiting, None)),
            (
                HealthPacing::Sending,
                picture(after, at(5_000), Changed::Yes),
                (HealthPacing::Sending, None),
            ),
            (awaiting, PacingEvent::Sent { at: after }, (awaiting, None)),
            (
                awaiting,
                picture(before, at(1_000), Changed::Yes),
                (awaiting, None),
            ),
            (
                awaiting,
                picture(sent, at(1_000), Changed::Yes),
                (awaiting, None),
            ),
            (
                awaiting,
                picture(after, at(1_000), Changed::No),
                (since, None),
            ),
            (since, send, (since, None)),
            (
                since,
                picture(after, at(1_499), Changed::Yes),
                (since, None),
            ),
            (since, picture(after, at(1_500), Changed::No), (since, None)),
            (
                since,
                picture(after, at(1_500), Changed::Yes),
                (HealthPacing::Sending, Some(SendHealth)),
            ),
        ];
        for (pacing, event, expected) in cases {
            assert_eq!(pacing.next(event), expected, "{pacing:?} {event:?}");
        }
    }
}
