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
    Refusal, Reply, Request, SavedLook, Sender, Snapshot, changes,
};
use engine_core::{HeldLight, Library, ShowFile, held_lights, light_changes};
use library::Shelf;
use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, mpsc, watch};
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
    let picture = Picture::of(engine.fresh().clone(), &first, &shelf, &shown);
    let front = Front {
        shelf: shelf.clone(),
        moments: taken,
        kept: VecDeque::from([first]),
        show: shown,
    };
    let (painted, pictures) = watch::channel(Arc::new(picture));
    let reader = tokio::spawn(read(engine, front, painted, stop.subscribe()));
    let api = Arc::new(Api {
        moments,
        patch,
        sender: Mutex::new(sender),
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
    sender: Mutex<Sender>,
    /// The front after each change, in the order of the changes: the reader pairs each
    /// snapshot with the one it was taken under.
    moments: mpsc::UnboundedSender<Moment>,
    /// The front's patch and looks after the last command, for the show file.
    patch: watch::Sender<show::Kept>,
    /// Turns true once, on `Stop`. Every connection and the server watch it.
    stop: watch::Sender<bool>,
    /// The newest picture, one per frame, the same for every connection.
    pictures: watch::Receiver<Arc<Picture>>,
    /// The fixture library, once it is read.
    shelf: watch::Receiver<Arc<Shelf>>,
}

/// A frame as every connection shows it: the engine's snapshot, and the library, the
/// project, the patch and the show file as they were when the snapshot was taken.
struct Picture {
    snapshot: Snapshot,
    library: Library,
    project: Project,
    patch: Arc<Patch>,
    looks: Arc<Looks>,
    show: ShowFile,
}

impl Picture {
    fn of(
        snapshot: Snapshot,
        moment: &Moment,
        shelf: &watch::Receiver<Arc<Shelf>>,
        show: &watch::Receiver<ShowFile>,
    ) -> Self {
        Self {
            snapshot,
            library: shelf.borrow().library(),
            project: moment.project,
            patch: Arc::clone(&moment.patch),
            looks: Arc::clone(&moment.looks),
            show: *show.borrow(),
        }
    }
}

/// The front's project, patch and looks once a change ended: revision `revision`, after
/// `sent` commands. A snapshot that read `revision` at its cutoff and applied exactly
/// `sent` commands was taken under them.
struct Moment {
    sent: u64,
    revision: u64,
    project: Project,
    patch: Arc<Patch>,
    looks: Arc<Looks>,
}

impl Moment {
    fn of(sender: &Sender) -> Self {
        Self {
            sent: sender.sent(),
            revision: sender.revision(),
            project: sender.project(),
            patch: sender.patch(),
            looks: sender.looks(),
        }
    }
}

/// The moment a snapshot was taken under, if it is known: the one of the `revision` it
/// read at its cutoff, with exactly its `applied` commands. `None` while the tick is part
/// way through the commands of one change, or the change is not in `kept` yet. Forgets
/// the moments older than it: the revision never goes back.
fn under(kept: &mut VecDeque<Moment>, applied: u64, revision: u64) -> Option<&Moment> {
    while kept.get(1).is_some_and(|next| next.revision <= revision) {
        kept.pop_front();
    }
    kept.front()
        .filter(|moment| moment.revision == revision && moment.sent == applied)
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
    fn picture(&mut self, snapshot: &Snapshot) -> Option<Picture> {
        while let Ok(moment) = self.moments.try_recv() {
            self.kept.push_back(moment);
        }
        let moment = under(&mut self.kept, snapshot.applied(), snapshot.revision())?;
        Some(Picture::of(
            snapshot.clone(),
            moment,
            &self.shelf,
            &self.show,
        ))
    }
}

/// The one reader of the engine: takes the newest snapshot twice a beat and shares it,
/// with the library, the project and the patch, when it is of a new frame. Until `Stop`,
/// then it gives the engine back.
async fn read(
    mut engine: Engine,
    mut front: Front,
    painted: watch::Sender<Arc<Picture>>,
    mut stopped: watch::Receiver<bool>,
) -> Engine {
    let mut poll = tokio::time::interval(engine::BEAT / 2);
    poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = stopped.wait_for(|stop| *stop) => return engine,
            _ = poll.tick() => {
                let snapshot = engine.latest();
                if snapshot.state.frame != painted.borrow().snapshot.state.frame
                    && let Some(picture) = front.picture(snapshot)
                {
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
    let mut subscription: Option<Arc<Picture>> = None;
    let mut stopped = api.stop.subscribe();
    loop {
        let reply = tokio::select! {
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
                changed(&mut pictures, &mut subscription)
            }
        };
        if let Some(reply) = reply
            && socket.send(Message::text(reply)).await.is_err()
        {
            return; // client gone
        }
    }
}

async fn answer(
    api: &Api,
    text: &str,
    pictures: &mut watch::Receiver<Arc<Picture>>,
    subscription: &mut Option<Arc<Picture>>,
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
        Request::Query => picture(&pictures.borrow()),
        Request::Stop => {
            api.stopping().await;
            json(&Reply::Stopping)
        }
        Request::Subscribe => {
            let now = Arc::clone(&pictures.borrow_and_update());
            let reply = picture(&now);
            *subscription = Some(now);
            reply
        }
        Request::Search { text } => api.shelf.borrow().found(text.as_str()),
        Request::Fixture { maker, model } => {
            api.shelf.borrow().fixture(maker.as_str(), model.as_str())
        }
        Request::Failures => api.shelf.borrow().failures(),
    }
}

/// How often a command prepared on a base another change replaced is prepared again,
/// before the client is told `Stale`.
const PREPARES: usize = 3;

/// Sends `command` to the front, with the library as it is read so far. A press is
/// prepared and admitted under the front at once: no thread hop between it and the next
/// frame. An edit ([`engine::edits`]) is prepared on a thread of the blocking pool,
/// without the front (a patch measures its show file, a look compiles), and admitted
/// under it: every other client is answered meanwhile. So is a device it needs opened.
/// The answer when it is taken, `Stopping` when the engine stops first: nothing changed.
async fn send(api: &Api, command: Command) -> Result<Reply<'static>, Refusal> {
    let shelf = Arc::clone(&api.shelf.borrow());
    let begun = if engine::edits(&command) {
        edit(api, command, &shelf).await
    } else {
        api.changing(|sender| sender.begin(command, shelf.fixtures()))
            .await
    };
    match begun {
        None => Ok(Reply::Stopping),
        Some(Ok(Begun::Opening(opening))) => {
            match tokio::task::spawn_blocking(move || opening.open()).await {
                Ok(opened) => match api.changing(|sender| sender.finish(opened)).await {
                    None => Ok(Reply::Stopping),
                    Some(finished) => finished.map(|()| Reply::Accepted),
                },
                Err(_) => Err(Refusal::CannotOpen), // the task was cancelled: the engine stops
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
async fn edit(api: &Api, command: Command, shelf: &Arc<Shelf>) -> Option<Result<Begun, Refusal>> {
    let mut begun = Some(Err(Refusal::Stale));
    for _ in 0..PREPARES {
        let base = api.sender.lock().await.base();
        let shelf = Arc::clone(shelf);
        let prepared =
            tokio::task::spawn_blocking(move || base.prepare(command, shelf.fixtures())).await;
        begun = match prepared {
            Ok(Ok(prepared)) => api.changing(|sender| sender.admit(prepared)).await,
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
    async fn changing<T>(&self, change: impl FnOnce(&mut Sender) -> T) -> Option<T> {
        let mut sender = self.sender.lock().await;
        if *self.stop.borrow() {
            return None;
        }
        let changed = change(&mut sender);
        sender.revise();
        // The reader is gone only once the server stops: nobody needs the moment then.
        let _ = self.moments.send(Moment::of(&sender));
        self.patch.send_if_modified(|kept| kept.follow(&sender));
        Some(changed)
    }

    /// Stops the engine: under the front, so every change admitted before is one the
    /// last save has, and none is admitted after.
    async fn stopping(&self) {
        let _front = self.sender.lock().await;
        self.stop.send_replace(true);
    }
}

fn picture(now: &Picture) -> Option<String> {
    let lights: Vec<HeldLight> = held_lights(now.snapshot.lights()).collect();
    json(&Reply::Picture {
        state: now.snapshot.state,
        project: now.project,
        library: now.library,
        patch: &now.patch.fixtures(),
        groups: &now.patch.groups(),
        looks: &now.looks.all(),
        show: now.show,
        universes: now.snapshot.universes(),
        lights: &lights,
    })
}

/// `Changed`: the channels and the lights that differ from the last picture this connection
/// sent, and the patch and the groups if the patch is not the one of that picture.
fn changed(
    pictures: &mut watch::Receiver<Arc<Picture>>,
    subscription: &mut Option<Arc<Picture>>,
) -> Option<String> {
    let last = subscription.as_mut()?;
    let now = Arc::clone(&pictures.borrow_and_update());
    let channels: Vec<ChannelLevel> =
        changes(last.snapshot.universes(), now.snapshot.universes()).collect();
    let lights: Vec<HeldLight> =
        light_changes(last.snapshot.lights(), now.snapshot.lights()).collect();
    let new = !Arc::ptr_eq(&last.patch, &now.patch);
    let fixtures: Option<Vec<Patched>> = new.then(|| now.patch.fixtures());
    let groups: Option<Vec<Grouped>> = new.then(|| now.patch.groups());
    let looks: Option<Vec<SavedLook>> =
        (!Arc::ptr_eq(&last.looks, &now.looks)).then(|| now.looks.all());
    let reply = json(&Reply::Changed {
        state: now.snapshot.state,
        project: now.project,
        library: now.library,
        patch: fixtures.as_deref(),
        groups: groups.as_deref(),
        looks: looks.as_deref(),
        show: now.show,
        channels: &channels,
        lights: &lights,
    });
    *last = now;
    reply
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
        let a = front.picture(&a).expect("A's moment is kept");
        let b = engine.fresh().clone();
        let b = front.picture(&b).expect("B's moment is kept");

        // (picture, saved looks, the play's look, light 1's colour)
        let cases = [
            (&a, vec![1], Some(1), Some(green)),
            (&b, vec![2], None, None),
        ];
        for (picture, looks, playing, colour) in cases {
            let saved: Vec<u16> = picture.looks.all().iter().map(|saved| saved.look).collect();
            assert_eq!(saved, looks);
            let plays: Vec<u16> = picture
                .snapshot
                .state
                .plays
                .iter()
                .map(|play| play.look)
                .collect();
            assert_eq!(plays.first().copied(), playing);
            let held = held_lights(picture.snapshot.lights()).find(|held| held.light == 1);
            assert_eq!(held.and_then(|held| held.colour), colour);
        }
        // Query: each picture as it is
        let query: Value = serde_json::from_str(&picture(&a).unwrap()).unwrap();
        assert_eq!(query["Picture"]["looks"][0]["look"], 1);
        assert_eq!(query["Picture"]["state"]["plays"][play.slot()]["look"], 1);
        // Subscribe: A delivered, then B's change carries B's looks
        let (paint, mut pictures) = watch::channel(Arc::new(a));
        let mut subscription = Some(Arc::clone(&pictures.borrow_and_update()));
        paint.send_replace(Arc::new(b));
        let changed: Value =
            serde_json::from_str(&changed(&mut pictures, &mut subscription).unwrap()).unwrap();
        let looks = &changed["Changed"]["looks"];
        assert_eq!(
            (looks[0]["look"].clone(), looks[1].clone()),
            (2.into(), Value::Null)
        );
        assert!(changed["Changed"]["state"]["plays"][play.slot()].is_null());
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
            let picture = front.picture(&snapshot).expect("its moment is kept");
            let shown = picture.looks.get(1).map(|saved| saved.name.as_str());
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
