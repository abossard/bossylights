//! The engine API on one WebSocket. What the messages are: `engine_core::Request` and
//! `engine_core::Reply`. This file only moves them.
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
pub mod show;

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::header::{ACCESS_CONTROL_ALLOW_ORIGIN, ORIGIN};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use engine::{
    API_VERSION, Begun, ChannelLevel, Command, Engine, Grouped, Looks, Patch, Patched, Project,
    ProjectView, Refusal, Reply, Request, SavedLook, Sender, Snapshot, changes,
};
use engine_core::{HeldLight, Library, ShowFile};
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
    // a show file keeps no declarations: it opens once the library is read
    let opened = match show.as_deref() {
        None => ShowFile::None,
        Some(path) => {
            let mut read = shelf.clone();
            let _ = read
                .wait_for(|shelf| !matches!(**shelf, Shelf::Reading))
                .await;
            let library = Arc::clone(&read.borrow());
            show::open(path, &mut sender, library.fixtures())
        }
    };
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
    let snapshot = engine.fresh();
    let mut health = health::Reader::new();
    health.observe(|output, sample| snapshot.health_sample(output, sample));
    let picture = Picture::of(
        snapshot.clone(),
        requested_at,
        &first,
        &shelf,
        &shown,
        health.view(),
    );
    let through = Arc::clone(&first.through);
    let held = Arc::clone(&first.held);
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
            held,
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
    /// The lights the grabs hold in this snapshot.
    held: Arc<[HeldLight]>,
}

impl Picture {
    fn of(
        snapshot: Snapshot,
        requested_at: Instant,
        moment: &Moment,
        shelf: &watch::Receiver<Arc<Shelf>>,
        show: &watch::Receiver<ShowFile>,
        health: &health::View,
    ) -> Self {
        let project = moment.project.at(&snapshot);
        Self {
            held: moment.held.at(snapshot.applied()),
            snapshot,
            requested_at,
            health: health.clone(),
            library: shelf.borrow().library(),
            project,
            patch: Arc::clone(&moment.patch),
            looks: Arc::clone(moment.rows.get_or_init(|| moment.looks.all().into())),
            source: Arc::downgrade(&moment.looks),
            show: *show.borrow(),
        }
    }
}

/// A revision covers every completed FIFO prefix from `sent` through `through`.
/// A performance command extends that range without retaining another project.
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
    /// The held lights after each grab of this revision.
    held: Arc<HeldLog>,
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
            held: Arc::new(HeldLog::new(sender.sent(), sender.held())),
        }
    }
}

/// The lights the grabs hold after each command a grab queued, by its number: the front
/// keeps its grabs, the tick has no lights. A picture shows those its snapshot applied.
struct HeldLog(std::sync::Mutex<VecDeque<(u64, Arc<[HeldLight]>)>>);

impl HeldLog {
    fn new(sent: u64, held: Arc<[HeldLight]>) -> Self {
        Self(std::sync::Mutex::new(VecDeque::from([(sent, held)])))
    }

    /// The lights held once the first `sent` commands are applied, if they changed.
    fn push(&self, sent: u64, held: Arc<[HeldLight]>) {
        let mut log = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if log.back().is_none_or(|(_, last)| !Arc::ptr_eq(last, &held)) {
            log.push_back((sent, held));
        }
    }

    /// The lights held in a snapshot that applied the first `applied` commands. Forgets
    /// the ones before: the reader never goes back.
    fn at(&self, applied: u64) -> Arc<[HeldLight]> {
        let mut log = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while log.get(1).is_some_and(|(sent, _)| *sent <= applied) {
            log.pop_front();
        }
        log.front()
            .map_or_else(|| Arc::from([]), |(_, held)| Arc::clone(held))
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
        health: &health::View,
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
    let mut observed_frame = painted.borrow().snapshot.state.frame;
    let mut requested_at = painted.borrow().requested_at;
    let mut next_request = Instant::now();
    loop {
        tokio::select! {
            _ = stopped.wait_for(|stop| *stop) => return engine,
            _ = poll.tick() => {
                let poll_started = Instant::now();
                let snapshot = engine.latest();
                let frame = snapshot.state.frame;
                // The front may need another poll to find this frame's project.
                if frame != observed_frame {
                    // latest takes the previous request's snapshot and asks for the next.
                    requested_at = std::mem::replace(&mut next_request, poll_started);
                    health.observe(|output, sample| snapshot.health_sample(output, sample));
                    observed_frame = frame;
                }
                let changed = {
                    let last = painted.borrow();
                    frame != last.snapshot.state.frame
                        || health.view().revision() != last.health.revision()
                };
                if changed && let Some(picture) = front.picture(snapshot, requested_at, health.view()) {
                    painted.send_replace(Arc::new(picture));
                }
            }
        }
    }
}

/// The web pages that may open the API, and why. A browser always sends the page's
/// origin; the shell, tests and scripts send none.
const ORIGINS: [&str; 4] = [
    "tauri://localhost",      // the app window on macOS and Linux
    "http://tauri.localhost", // the app window on Windows
    "http://127.0.0.1:5174",  // the UI in a browser, `npm run ui`
    "http://127.0.0.1:5299",  // the same UI under the end-to-end tests, `playwright.config.ts`
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

/// One connection: answers each request; once subscribed, sends what changed with every
/// new snapshot. Never waits on the tick thread, the tick thread never waits on a client.
async fn talk(mut socket: WebSocket, api: Arc<Api>) {
    let mut pictures = api.pictures.clone();
    // What a subscribed connection sent last.
    let mut subscription: Option<Subscription> = None;
    let mut stopped = api.stop.subscribe();
    loop {
        let health = subscription.as_mut().and_then(|sub| {
            let now = Arc::clone(&pictures.borrow());
            sub.health(&now)
        });
        let reply = if health.is_some() {
            health
        } else {
            tokio::select! {
                () = async { let _ = stopped.wait_for(|stop| *stop).await; } => {
                    let _ = socket.send(Message::Close(None)).await;
                    return;
                }
                message = socket.recv() => match message {
                    Some(Ok(Message::Text(text))) => answer(&api, &text, &mut pictures, &mut subscription).await,
                    Some(Ok(Message::Binary(_))) => json(&Reply::Refused { reason: Refusal::NotACommand }),
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => None,
                    Some(Ok(Message::Close(_)) | Err(_)) | None => return,
                },
                Ok(()) = pictures.changed(), if subscription.is_some() => {
                    let now = Arc::clone(&pictures.borrow_and_update());
                    subscription.as_mut().and_then(|sub| sub.changed(now))
                }
            }
        };
        if let Some(reply) = reply {
            if socket.send(Message::text(reply)).await.is_err() {
                return; // client gone
            }
            if let Some(sub) = &mut subscription {
                sub.sent();
            }
        }
    }
}

async fn answer(
    api: &Arc<Api>,
    text: &str,
    pictures: &mut watch::Receiver<Arc<Picture>>,
    subscription: &mut Option<Subscription>,
) -> Option<String> {
    let Ok(request) = serde_json::from_str::<Request>(text) else {
        return json(&Reply::Refused {
            reason: Refusal::NotACommand,
        });
    };
    match request {
        Request::Send { command } => json(&match send(api, command).await {
            Ok(reply) => reply,
            Err(reason) => Reply::Refused { reason },
        }),
        Request::Query => {
            let now = Arc::clone(&pictures.borrow());
            picture(&now)
        }
        Request::Stop => {
            api.stopping().await;
            json(&Reply::Stopping)
        }
        Request::Subscribe => {
            let now = Arc::clone(&pictures.borrow_and_update());
            let reply = picture(&now);
            *subscription = Some(Subscription::new(now));
            reply
        }
        Request::Search { text } => api.shelf.borrow().found(text.as_str()),
        Request::Fixture { maker, model } => {
            api.shelf.borrow().fixture(maker.as_str(), model.as_str())
        }
        Request::Failures => api.shelf.borrow().failures(),
    }
}

/// The only writer of the current revision's FIFO range.
struct Publisher {
    sender: Sender,
    through: Arc<AtomicU64>,
    /// The held lights of the newest moment.
    held: Arc<HeldLog>,
}

/// How often a command prepared on a base another change replaced is prepared again,
/// before the client is told `Stale`.
const PREPARES: usize = 3;

/// An output writer may wait for its previous route to finish. Other writers
/// keep using the front, including a performer sending a press to either run.
const CUTOVER_WAIT: Duration = Duration::from_secs(10);

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
    let output_edit = matches!(
        command,
        Command::SetOutput { .. } | Command::RemoveOutput { .. }
    );
    let begun = if output_edit || engine::edits(&command) {
        edit(api, command, &shelf, &mut credit).await
    } else {
        api.changing(&mut credit, |sender| {
            sender.begin(command, shelf.fixtures())
        })
        .await
    };
    match begun {
        None => Ok(Reply::Stopping),
        Some(Ok(Begun::Opening(opening))) => {
            let api = Arc::clone(api);
            let mut stopped = api.stop.subscribe();
            // Keep the opener through the handoff. A busy blocking pool cannot
            // put another job between publication and delivery of the device.
            let worker = tokio::task::spawn_blocking(move || {
                let opened = opening.open();
                let publish = api.changing(&mut credit, |sender| sender.publish_output(opened));
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
                answer = worker => answer.unwrap_or(Ok(Reply::Stopping)),
            }
        }
        Some(Ok(Begun::Queued)) => Ok(Reply::Accepted),
        Some(Ok(Begun::Patched { fixture })) => Ok(Reply::Patched { fixture }),
        Some(Ok(Begun::Started { play })) => Ok(Reply::Started { play }),
        Some(Ok(Begun::Grabbed { grab })) => Ok(Reply::Grabbed { grab }),
        Some(Err(reason)) => Err(reason),
    }
}

/// Prepares the edit `command` on the blocking pool and admits it, again while another
/// change replaced its base, [`PREPARES`] times at most. `None` once the engine stops.
async fn edit(
    api: &Api,
    command: Command,
    shelf: &Arc<Shelf>,
    opening_credit: &mut Option<OwnedSemaphorePermit>,
) -> Option<Result<Begun, Refusal>> {
    let mut begun = Some(Err(Refusal::Stale));
    let waiting_since = Instant::now();
    for _ in 0..PREPARES {
        // T12. Read the base and the state of this output under the same lock.
        // Refusal and stop can both end the wait while the engine clock is held.
        let base = loop {
            let front = api.sender.lock().await;
            if *api.stop.borrow() {
                return None;
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
                return Some(Err(Refusal::Opening));
            }
            tokio::time::sleep(engine::BEAT).await;
        };
        // Bound in-flight edits too. A full history refuses before compiling.
        let mut credit = match reserve(&api.history, REVISION_BYTES) {
            Ok(credit) => credit,
            Err(reason) => return Some(Err(reason)),
        };
        let shelf = Arc::clone(shelf);
        let history = Arc::clone(&api.history);
        let prepared = tokio::task::spawn_blocking(move || {
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
                admitted
            }
            // refused before the engine stops, or answered `Stopping` as an admitted one is
            Ok(Err(reason)) => (!*api.stop.borrow()).then_some(Err(reason)),
            Err(_) => None, // the task was cancelled: the engine stops
        };
        if !matches!(begun, Some(Err(Refusal::Stale))) {
            break;
        }
    }
    begun
}

impl Api {
    /// Changes the front with `change`, and tells the reader the front as `change` left
    /// it, before another change: each moment is one the engine was in. The show file
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
        let sender = &mut publisher.sender;
        let before = sender.project_for_snapshot();
        let changed = change(sender);
        let edited = self.patch.send_if_modified(|kept| kept.follow(sender));
        if edited || !before.same_outputs(&sender.project_for_snapshot()) {
            sender.revise();
            let mut moment = Moment::of(sender);
            moment.credit = credit.take();
            debug_assert!(moment.credit.is_some());
            publisher.through = Arc::clone(&moment.through);
            publisher.held = Arc::clone(&moment.held);
            // The reader is gone only once the server stops.
            let _ = self.moments.send(moment);
        } else {
            let (sent, held) = (sender.sent(), sender.held());
            publisher.held.push(sent, held);
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
        state: now.snapshot.state,
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

/// A subscription Picture or Health may finish sending long after its pictured wake.
/// A snapshot requested after delivery gives an upper bound on the engine time of the
/// send. The host clock only orders sends and snapshot requests.
enum HealthPacing {
    Sending,
    AwaitingSample(Instant),
    Since(Duration),
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
        if matches!(self.pacing, HealthPacing::Sending) {
            self.pacing = HealthPacing::AwaitingSample(Instant::now());
        }
    }

    fn health(&mut self, picture: &Picture) -> Option<String> {
        let view = &picture.health;
        let now = picture.snapshot.state.now;
        if let HealthPacing::AwaitingSample(sent) = self.pacing
            && picture.requested_at > sent
        {
            self.pacing = HealthPacing::Since(now);
        }
        let HealthPacing::Since(sent) = self.pacing else {
            return None;
        };
        let since = now.saturating_sub(sent);
        if self.revision == view.revision() || since < Duration::from_millis(500) {
            return None;
        }
        let (events, missed) = view.events_since(self.cursor);
        self.cursor = view.end();
        self.revision = view.revision();
        self.pacing = HealthPacing::Sending;
        json(&Reply::Health {
            health: view.health(),
            events: &events,
            missed,
        })
    }

    fn changed(&mut self, now: Arc<Picture>) -> Option<String> {
        let last = std::mem::replace(&mut self.last, now);
        if last.snapshot.state.frame == self.last.snapshot.state.frame {
            return None;
        }
        changed(&last, &self.last)
    }
}

/// Every light whose held parts differ between `before` and `after`, as `after` holds
/// them, by number: a light let go holds nothing.
fn held_changes(before: &[HeldLight], after: &[HeldLight]) -> Vec<HeldLight> {
    if core::ptr::eq(before, after) {
        return Vec::new();
    }
    let let_go = |light| HeldLight {
        light,
        intensity: None,
        colour: None,
    };
    let (mut old, mut new) = (before.iter().peekable(), after.iter().peekable());
    let mut changed = Vec::new();
    loop {
        match (old.peek(), new.peek()) {
            (None, None) => return changed,
            (Some(o), Some(n)) if o.light == n.light => {
                if o != n {
                    changed.push(**n);
                }
                old.next();
                new.next();
            }
            (Some(o), None) => {
                changed.push(let_go(o.light));
                old.next();
            }
            (Some(o), Some(n)) if o.light < n.light => {
                changed.push(let_go(o.light));
                old.next();
            }
            (_, Some(n)) => {
                changed.push(**n);
                new.next();
            }
        }
    }
}

/// The channels and lights that differ from this connection's last frame.
fn changed(last: &Picture, now: &Picture) -> Option<String> {
    let channels: Vec<ChannelLevel> =
        changes(last.snapshot.universes(), now.snapshot.universes()).collect();
    let lights = held_changes(&last.held, &now.held);
    let new = !Arc::ptr_eq(&last.patch, &now.patch);
    let fixtures: Option<Vec<Patched>> = new.then(|| now.patch.fixtures());
    let groups: Option<Vec<Grouped>> = new.then(|| now.patch.groups());
    let looks = (!Weak::ptr_eq(&last.source, &now.source)).then_some(now.looks.as_ref());
    json(&Reply::Changed {
        state: now.snapshot.state,
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
            },
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
        }) else {
            panic!("look 1 plays");
        };
        // revision A: look 1 green, its play takes the new plan. A's snapshot is taken
        change(step(1, green));
        let a = engine.fresh().clone();
        // revision B overtakes it before it is painted: the play released, look 1
        // removed, look 2 saved
        change(Command::ReleaseHold { play });
        change(Command::RemoveLook { look: 1 });
        change(step(2, blue));
        let health = health::View::default();
        let a = front
            .picture(&a, Instant::now(), &health)
            .expect("A's moment is kept");
        let b = engine.fresh().clone();
        let b = front
            .picture(&b, Instant::now(), &health)
            .expect("B's moment is kept");

        // (picture, saved looks, the play's look, light 1's bytes: the play's colour)
        let cases = [
            (&a, vec![1], Some(1), [0, 255, 0]),
            (&b, vec![2], None, [0, 0, 0]),
        ];
        for (picture, looks, playing, bytes) in cases {
            let saved: Vec<u16> = picture.looks.iter().map(|saved| saved.look).collect();
            assert_eq!(saved, looks);
            let plays: Vec<u16> = picture
                .snapshot
                .state
                .plays
                .iter()
                .map(|play| play.look)
                .collect();
            assert_eq!(plays.first().copied(), playing);
            assert_eq!(picture.snapshot.shown()[0].0[..3], bytes);
            assert!(picture.held.is_empty(), "nothing grabbed");
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
            engine.fresh().clone(),
            Instant::now(),
            &Moment::of(&sender),
            &library::start(None),
            &watch::channel(ShowFile::None).1,
            &health::View::default(),
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
            },
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
                .picture(&snapshot, Instant::now(), &health::View::default())
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
}
