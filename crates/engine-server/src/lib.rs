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
    API_VERSION, Begun, ChannelLevel, Command, Engine, Grouped, Patch, Patched, Project, Refusal,
    Reply, Request, Sender, Snapshot, changes,
};
use engine_core::{Library, ShowFile};
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
    let (patch, _) = watch::channel(sender.patch());
    let keeper = show
        .filter(|_| opened.saves())
        .map(|path| tokio::spawn(show::keep(path, patch.subscribe(), kept, stop.subscribe())));
    let first = Moment::of(&sender);
    // A snapshot of a wake after every command queued so far: taken under `first`.
    let picture = Picture::of(*engine.fresh(), &first, &shelf, &shown);
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
    api.stop.send_replace(true);
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
    /// The front's patch after the last command, for the show file.
    patch: watch::Sender<Arc<Patch>>,
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
            show: *show.borrow(),
        }
    }
}

/// The front's project and patch once it had queued `sent` commands. A snapshot that
/// applied exactly `sent` commands was taken under them.
struct Moment {
    sent: u64,
    project: Project,
    patch: Arc<Patch>,
}

impl Moment {
    fn of(sender: &Sender) -> Self {
        Self {
            sent: sender.sent(),
            project: sender.project(),
            patch: sender.patch(),
        }
    }
}

/// The moment a snapshot that applied `applied` commands was taken under, if it is known:
/// the newest one at exactly `applied`. `None` while the tick is part way through the
/// commands of one change, or the change is not in `kept` yet. Forgets the moments
/// older than it: `applied` never goes back.
fn under(kept: &mut VecDeque<Moment>, applied: u64) -> Option<&Moment> {
    while kept.get(1).is_some_and(|next| next.sent <= applied) {
        kept.pop_front();
    }
    kept.front().filter(|moment| moment.sent == applied)
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
        let moment = under(&mut self.kept, snapshot.applied())?;
        Some(Picture::of(*snapshot, moment, &self.shelf, &self.show))
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
            Ok(Some(fixture)) => Reply::Patched { fixture },
            Ok(None) => Reply::Accepted,
            Err(reason) => Reply::Refused { reason },
        }),
        Request::Query => picture(&pictures.borrow()),
        Request::Stop => {
            api.stop.send_replace(true);
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

/// Sends `command` to the front, with the library as it is read so far. A device it needs
/// is opened on a thread of the blocking pool, without the front: every other client is
/// answered meanwhile. The number of a patched fixture.
async fn send(api: &Api, command: Command) -> Result<Option<u16>, Refusal> {
    let shelf = Arc::clone(&api.shelf.borrow());
    let begun = api
        .changing(|sender| sender.begin(command, shelf.fixtures()))
        .await;
    match begun {
        Ok(Begun::Opening(opening)) => {
            match tokio::task::spawn_blocking(move || opening.open()).await {
                Ok(opened) => api.changing(|sender| sender.finish(opened)).await.map(|()| None),
                Err(_) => Err(Refusal::CannotOpen), // the task was cancelled: the engine stops
            }
        }
        Ok(Begun::Queued) => Ok(None),
        Ok(Begun::Patched { fixture }) => Ok(Some(fixture)),
        Err(reason) => Err(reason),
    }
}

impl Api {
    /// Changes the front with `change`, and tells the reader and the show file the front
    /// as `change` left it, before another change: each moment is one the engine was in.
    async fn changing<T>(&self, change: impl FnOnce(&mut Sender) -> T) -> T {
        let mut sender = self.sender.lock().await;
        let changed = change(&mut sender);
        // The reader is gone only once the server stops: nobody needs the moment then.
        let _ = self.moments.send(Moment::of(&sender));
        self.patch.send_replace(sender.patch());
        changed
    }
}

fn picture(now: &Picture) -> Option<String> {
    json(&Reply::Picture {
        state: now.snapshot.state,
        project: now.project,
        library: now.library,
        patch: &now.patch.fixtures(),
        groups: &now.patch.groups(),
        show: now.show,
        universes: now.snapshot.universes(),
    })
}

/// `Changed`: the channels that differ from the last picture this connection sent, and
/// the patch and the groups if the patch is not the one of that picture.
fn changed(
    pictures: &mut watch::Receiver<Arc<Picture>>,
    subscription: &mut Option<Arc<Picture>>,
) -> Option<String> {
    let last = subscription.as_mut()?;
    let now = Arc::clone(&pictures.borrow_and_update());
    let channels: Vec<ChannelLevel> =
        changes(last.snapshot.universes(), now.snapshot.universes()).collect();
    let new = !Arc::ptr_eq(&last.patch, &now.patch);
    let fixtures: Option<Vec<Patched>> = new.then(|| now.patch.fixtures());
    let groups: Option<Vec<Grouped>> = new.then(|| now.patch.groups());
    let reply = json(&Reply::Changed {
        state: now.snapshot.state,
        project: now.project,
        library: now.library,
        patch: fixtures.as_deref(),
        groups: groups.as_deref(),
        show: now.show,
        channels: &channels,
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
    use engine::Universes;

    #[test]
    fn a_snapshot_is_paired_only_with_the_moment_at_exactly_its_applied_commands() {
        let (_engine, sender) = Engine::start(Universes::new(1).unwrap()).unwrap();
        // (sent of each kept moment, applied, sent of the moment found, sent of those kept after)
        let cases = [
            (vec![0], 0, Some(0), vec![0]),
            (vec![0], 1, None, vec![0]), // queued, its moment not here yet
            (vec![0, 2], 1, None, vec![0, 2]), // part way through one change
            (vec![0, 2], 2, Some(2), vec![2]),
            (vec![0, 2, 2], 2, Some(2), vec![2]), // a change that queued nothing: the newest
            (vec![0, 2, 5], 3, None, vec![2, 5]),
            (vec![0, 2, 5], 7, None, vec![5]),
        ];
        for (sents, applied, found, left) in cases {
            let mut kept: VecDeque<Moment> = sents
                .iter()
                .map(|&sent| Moment {
                    sent,
                    ..Moment::of(&sender)
                })
                .collect();
            let moment = under(&mut kept, applied).map(|moment| moment.sent);
            assert_eq!(moment, found, "{sents:?} at {applied}");
            let after: Vec<u64> = kept.iter().map(|moment| moment.sent).collect();
            assert_eq!(after, left, "{sents:?} at {applied}");
        }
    }
}
