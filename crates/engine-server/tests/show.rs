//! End to end with the engine process and `--show <file>`: patch, stop, start again
//! without the library: the patch and the groups are there and light up on DDP. A file
//! the engine cannot open is left alone and the picture says why. A save that cannot be
//! written leaves the file as it was, and the picture says so until the next save. A
//! change that would take the file over its most bytes is refused, and the file kept. So
//! is a step over a patch that leaves the looks no room. A refused edit saves nothing.
//! An edit made before the keeper first looks is saved.

mod common;

use common::taken;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tokio::net::{TcpStream, UdpSocket};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A show file of this test run, not there yet.
fn show_file(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "bossylights-show-{}-{name}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(saving(&path));
    path
}

fn saving(path: &Path) -> PathBuf {
    engine_server::show::saving(path)
}

/// An engine process, killed when the test ends however it ends.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts the engine on a port the system gives with the show file `show`, and the test
/// definitions when `library`. Returns once it listens, with the address it listens on.
fn engine(show: &Path, library: bool) -> (Running, String) {
    let definitions =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixture-library/tests/definitions");
    let mut command = Command::new(env!("CARGO_BIN_EXE_bossylights-engine"));
    command
        .args(["--listen", "127.0.0.1:0"])
        .arg("--show")
        .arg(show);
    if library {
        command.arg("--fixtures").arg(definitions);
    }
    let mut running = Running(
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut line = String::new();
    BufReader::new(running.0.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    (running, listening_on(&line))
}

/// The address in the engine's "listening on http://<address>, ..." line.
fn listening_on(line: &str) -> String {
    line.split_once("listening on http://")
        .and_then(|(_, rest)| rest.split_once(','))
        .map(|(address, _)| address.to_string())
        .unwrap_or_else(|| panic!("not listening: {line}"))
}

/// Sends `Stop` and waits at most 10 s for the process to end well.
async fn stop(socket: &mut Socket, Running(engine): &mut Running) {
    assert_eq!(answer_to(socket, &json!("Stop")).await, json!("Stopping"));
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = engine.try_wait().unwrap() {
            assert!(status.success(), "{status}");
            return;
        }
        assert!(Instant::now() < until, "the engine did not end within 10 s");
        thread::sleep(Duration::from_millis(50)); // poll
    }
}

async fn connect(listen: &str) -> Socket {
    let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{listen}/api"))
        .await
        .unwrap();
    socket
}

/// Sends `request`, returns the next message that is not a `Changed`.
async fn answer_to(socket: &mut Socket, request: &Value) -> Value {
    socket
        .send(Message::text(request.to_string()))
        .await
        .unwrap();
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("a message within 5 s")
            .unwrap()
            .unwrap();
        let reply: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
        if reply.get("Changed").is_none() {
            return reply;
        }
    }
}

async fn command(socket: &mut Socket, command: Value) -> Value {
    answer_to(socket, &json!({"Send": {"command": command}})).await
}

/// Asks until the picture shows `wanted` under `key`: a picture is of a frame, and the
/// frame after a command may not be taken yet. The picture.
async fn until_picture(socket: &mut Socket, key: &str, wanted: &Value) -> Value {
    let started = Instant::now();
    loop {
        let picture = answer_to(socket, &json!("Query")).await["Picture"].clone();
        if picture[key] == *wanted {
            return picture;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{key} {}, wanted {wanted}",
            picture[key]
        );
    }
}

async fn until_show(socket: &mut Socket, wanted: &Value) -> Value {
    until_picture(socket, "show", wanted).await
}

fn par(name: &str, address: u16) -> Value {
    json!({"Patch": {
        "name": name, "maker": "Generic", "model": "Generic RGB", "mode": "RGB",
        "at": {"universe": 1, "address": address},
    }})
}

fn strip(name: &str, address: u16) -> Value {
    json!({"PatchStrip": {
        "name": name, "pixels": 3, "color": "Rgb", "at": {"universe": 1, "address": address},
    }})
}

/// Patches `par`, again until the library is read. The reply.
async fn patch_once_read(socket: &mut Socket, par: Value) -> Value {
    let started = Instant::now();
    loop {
        let reply = command(socket, par.clone()).await;
        if reply != json!({"Refused": {"reason": "NoSuchFixture"}}) {
            return reply;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "no library");
    }
}

/// Waits until universe 1 from address 37 is `bytes` on the DDP socket.
async fn until_ddp(udp: &UdpSocket, bytes: &[u8]) {
    let started = Instant::now();
    let mut packet = vec![0; 1500];
    let mut seen = None;
    loop {
        if let Ok(Ok(size)) =
            tokio::time::timeout(Duration::from_millis(100), udp.recv(&mut packet)).await
        {
            let end = 10 + 36 + bytes.len();
            seen = (size >= end).then(|| packet[10 + 36..end].to_vec());
        }
        if seen.as_deref() == Some(bytes) {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "from 37: {seen:?}, wanted {bytes:?}"
        );
    }
}

#[tokio::test]
async fn patch_stop_start_the_patch_and_groups_are_there_and_light_up_without_the_library() {
    let file = show_file("restart");
    let (mut first, listen) = engine(&file, true);
    let mut socket = connect(&listen).await;
    until_show(&mut socket, &json!("Kept")).await;
    assert_eq!(
        patch_once_read(&mut socket, par("PAR 1", 37)).await,
        json!({"Patched": {"fixture": 1}})
    );
    for (n, address) in [(2, 40), (3, 43)] {
        assert_eq!(
            command(&mut socket, par(&format!("PAR {n}"), address)).await,
            json!({"Patched": {"fixture": n}})
        );
    }
    // a hole: fixtures 1 and 3 on lights 1 and 3
    let unpatch = json!({"Unpatch": {"fixture": 2}});
    assert_eq!(command(&mut socket, unpatch).await, json!("Accepted"));
    let front = json!({"SetGroup": {"name": "front", "members": [
        {"Fixture": {"fixture": 3}}, {"Fixture": {"fixture": 1}},
    ]}});
    assert_eq!(command(&mut socket, front).await, json!("Accepted"));
    let groups = json!([{"name": "front", "members": [
        {"Fixture": {"fixture": 3}}, {"Fixture": {"fixture": 1}},
    ]}]);
    let before = until_picture(&mut socket, "groups", &groups).await;
    stop(&mut socket, &mut first).await;
    assert!(file.exists(), "saved");
    assert!(!saving(&file).exists(), "the new file was renamed");

    let (mut second, listen) = engine(&file, false);
    let mut socket = connect(&listen).await;
    let after = until_show(&mut socket, &json!("Kept")).await;
    assert_eq!(after["patch"], before["patch"]);
    assert_eq!(after["groups"], before["groups"]);
    assert_eq!(
        after["patch"].as_array().unwrap().len(),
        2,
        "{}",
        after["patch"]
    );

    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let ddp = json!({"Ddp": {"address": udp.local_addr().unwrap().to_string(), "color": "Rgb"}});
    let set = json!({"SetOutput": {
        "output": 1, "kind": ddp, "universes": {"first": 1, "count": 1}, "rate": null,
    }});
    assert_eq!(command(&mut socket, set).await, json!("Accepted"));
    let group = json!({"Group": {"group": "front"}});
    let full = json!({"SetIntensity": {"target": group, "intensity": 65535}});
    let red = json!({"SetColour": {"target": group, "red": 65535, "green": 0, "blue": 0}});
    assert!(taken(&command(&mut socket, full).await));
    assert!(taken(&command(&mut socket, red).await));
    until_ddp(&udp, &[255, 0, 0, 0, 0, 0, 255, 0, 0]).await;
    stop(&mut socket, &mut second).await;
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn a_file_the_engine_cannot_open_is_left_alone_and_the_picture_says_why() {
    let mode = r#"{"name":"RGB","channels":[],"pixels":[],"layout":null,"degrees":null}"#;
    let rules =
        r#""rules":{"brightness":"Latest","manual_end":"UntilRelease","other_end":"Reveal"}"#;
    let empty = format!(
        r#"{{"version":{},"patch":[],"groups":[],"looks":[],{rules},"modes":{{}}}}"#,
        show_file::VERSION
    );
    let past = format!(
        r#"{{"version":{},"patch":[{{"fixture":1,"name":"PAR","mode":"m","at":{{"universe":9,"address":1}},"first":1}}],"groups":[],"looks":[],{rules},"modes":{{"m":{{"maker":"Generic","model":"Generic RGB","mode":{mode}}}}}}}"#,
        show_file::VERSION
    );
    // (case, the file, why)
    let cases = [
        (
            "newer",
            format!(
                r#"{{"version": {}, "patch": [], "outputs": []}}"#,
                show_file::VERSION + 1
            ),
            json!({"Newer": {"version": show_file::VERSION + 1}}),
        ),
        (
            "older",
            empty.replace(
                &format!(r#""version":{}"#, show_file::VERSION),
                r#""version":5"#,
            ),
            json!({"Older": {"version": 5}}),
        ),
        (
            "broken",
            "not a show file".to_string(),
            json!({"Broken": {"line": 1, "column": 2}}),
        ),
        (
            "past_the_universes",
            past,
            json!({"Refused": {"reason": {"NoSuchUniverse": {"universes": 4}}}}),
        ),
        (
            "one_byte_over_the_most",
            empty.clone() + &" ".repeat(show_file::MOST_BYTES + 1 - empty.len()),
            json!({"TooBig": {"most": show_file::MOST_BYTES}}),
        ),
    ];
    for (case, text, why) in cases {
        let file = show_file(case);
        std::fs::write(&file, &text).unwrap();
        let (mut running, listen) = engine(&file, false);
        let mut socket = connect(&listen).await;
        let picture = until_show(&mut socket, &json!({"LeftAlone": {"why": why}})).await;
        assert_eq!(picture["patch"], json!([]), "{case}");
        // a change is not saved
        assert_eq!(
            command(&mut socket, strip("Strip", 1)).await,
            json!({"Patched": {"fixture": 1}}),
            "{case}"
        );
        stop(&mut socket, &mut running).await;
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text, "{case}");
        assert!(!saving(&file).exists(), "{case}");
        let _ = std::fs::remove_file(&file);
    }
}

#[tokio::test]
async fn a_save_that_cannot_be_written_leaves_the_file_as_it_was_until_the_next_save() {
    let file = show_file("not_saved");
    let (mut running, listen) = engine(&file, false);
    let mut socket = connect(&listen).await;
    until_show(&mut socket, &json!("Kept")).await;
    assert_eq!(
        command(&mut socket, strip("A", 1)).await,
        json!({"Patched": {"fixture": 1}})
    );
    until_show(&mut socket, &json!("Kept")).await;
    let started = Instant::now();
    while !std::fs::read_to_string(&file).is_ok_and(|text| text.contains("\"A\"")) {
        assert!(started.elapsed() < Duration::from_secs(10), "A not saved");
        tokio::task::yield_now().await;
    }
    let saved = std::fs::read_to_string(&file).unwrap();

    // the new file cannot be made: a folder has its name
    std::fs::create_dir(saving(&file)).unwrap();
    assert_eq!(
        command(&mut socket, strip("B", 20)).await,
        json!({"Patched": {"fixture": 2}})
    );
    until_show(&mut socket, &json!("NotSaved")).await;
    assert_eq!(std::fs::read_to_string(&file).unwrap(), saved);

    std::fs::remove_dir(saving(&file)).unwrap();
    assert_eq!(
        command(&mut socket, strip("C", 40)).await,
        json!({"Patched": {"fixture": 3}})
    );
    until_show(&mut socket, &json!("Kept")).await;
    stop(&mut socket, &mut running).await;
    let text = std::fs::read_to_string(&file).unwrap();
    for name in ["\"A\"", "\"B\"", "\"C\""] {
        assert!(text.contains(name), "{name}: {text}");
    }
    let _ = std::fs::remove_file(&file);
}

/// A folder this test run can write to and empty, not there yet.
#[cfg(unix)]
fn show_folder(name: &str) -> PathBuf {
    let folder =
        std::env::temp_dir().join(format!("bossylights-show-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir(&folder).unwrap();
    folder
}

/// Waits at most 10 s for `file` to hold `name`.
async fn until_saved(file: &Path, name: &str) {
    let started = Instant::now();
    while !std::fs::read_to_string(file).is_ok_and(|text| text.contains(name)) {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{name} not saved"
        );
        tokio::task::yield_now().await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_save_whose_folder_cannot_be_synced_is_not_saved_until_the_next_save() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |bits| std::fs::Permissions::from_mode(bits);
    let folder = show_folder("unsynced");
    let file = folder.join("show.json");
    let (mut running, listen) = engine(&file, false);
    let mut socket = connect(&listen).await;
    until_show(&mut socket, &json!("Kept")).await;
    assert_eq!(
        command(&mut socket, strip("A", 1)).await,
        json!({"Patched": {"fixture": 1}})
    );
    until_saved(&file, "\"A\"").await;

    // files can be made and renamed in the folder, the folder cannot be opened to sync it
    std::fs::set_permissions(&folder, mode(0o300)).unwrap();
    assert_eq!(
        command(&mut socket, strip("B", 20)).await,
        json!({"Patched": {"fixture": 2}})
    );
    until_show(&mut socket, &json!("NotSaved")).await;
    std::fs::set_permissions(&folder, mode(0o700)).unwrap();
    // renamed, but not safe from a power cut
    until_saved(&file, "\"B\"").await;

    assert_eq!(
        command(&mut socket, strip("C", 40)).await,
        json!({"Patched": {"fixture": 3}})
    );
    until_show(&mut socket, &json!("Kept")).await;
    stop(&mut socket, &mut running).await;
    assert!(std::fs::read_to_string(&file).unwrap().contains("\"C\""));
    let _ = std::fs::remove_dir_all(&folder);
}

#[cfg(unix)]
#[tokio::test]
async fn a_file_of_a_terabyte_is_not_read_whole_and_left_alone() {
    let folder = show_folder("terabyte");
    let file = folder.join("show.json");
    // sparse: no room on the disk, but read whole it would not end in the 10 s below
    let terabyte = 1 << 40;
    std::fs::File::create(&file)
        .unwrap()
        .set_len(terabyte)
        .unwrap();
    let (mut running, listen) = engine(&file, false);
    let mut socket = connect(&listen).await;
    let why = json!({"TooBig": {"most": show_file::MOST_BYTES}});
    until_show(&mut socket, &json!({"LeftAlone": {"why": why}})).await;
    stop(&mut socket, &mut running).await;
    assert_eq!(std::fs::metadata(&file).unwrap().len(), terabyte);
    let _ = std::fs::remove_dir_all(&folder);
}

#[tokio::test]
async fn a_change_that_would_take_the_file_over_the_most_is_refused_and_the_file_kept() {
    let file = show_file("full");
    let (mut first, listen) = engine(&file, true);
    let mut socket = connect(&listen).await;
    until_show(&mut socket, &json!("Kept")).await;
    assert_eq!(
        patch_once_read(&mut socket, par("PAR 1", 37)).await,
        json!({"Patched": {"fixture": 1}})
    );
    stop(&mut socket, &mut first).await;
    // the red channel's name longer, so the patch is at the most it may take: the rest
    // of the file is the room the looks may take
    let text = std::fs::read_to_string(&file).unwrap();
    let most = show_file::LIMIT.most;
    let red = "\"name\": \"Red\"";
    assert!(text.contains(red), "{text}");
    let longer = format!("\"name\": \"Red{}\"", "x".repeat(most - text.len()));
    std::fs::write(&file, text.replacen(red, &longer, 1)).unwrap();

    let (mut second, listen) = engine(&file, false);
    let mut socket = connect(&listen).await;
    until_show(&mut socket, &json!("Kept")).await;
    let rename = |name: &str| json!({"Rename": {"fixture": 1, "name": name}});
    let full = json!({"Refused": {"reason": {"ShowFileFull": {"most": most}}}});
    assert_eq!(command(&mut socket, rename("PAR 12")).await, full);
    assert_eq!(
        command(&mut socket, rename("PAR 2")).await,
        json!("Accepted")
    );
    stop(&mut socket, &mut second).await;
    let saved = std::fs::read_to_string(&file).unwrap();
    assert_eq!(saved.len(), most);
    assert!(saved.contains("\"PAR 2\""));
    let _ = std::fs::remove_file(&file);
}

/// A step for fixture 1 in red, saved as look `look` named `name`.
fn red_step(look: u16, name: &str) -> Value {
    json!({"PutStep": {"look": look, "name": name, "step": {
        "target": {"Fixture": {"fixture": 1}},
        "values": {"intensity": null, "colour": {"red": 65535, "green": 0, "blue": 0},
            "position": null, "zoom": null, "gobos": [null, null, null]},
    }}})
}

#[test]
fn patch_and_sequence_edits_share_the_bytes_above_the_step_reserve() {
    use engine_core::{LOOKS, SavedLook, Sequence};
    let (first, mut sender) = engine::Engine::start(engine::Universes::new(1).unwrap()).unwrap();
    let empty = sender.patch();
    let library = [fixture_library::parse("Generic-Generic-RGB.qxf", GENERIC_RGB).unwrap()];
    sender
        .begin(serde_json::from_value(par("PAR", 1)).unwrap(), &library)
        .unwrap();
    let short = show_file::write(&sender.patch(), &sender.looks()).unwrap();
    let sequence = |name| {
        json!({ "look": 2, "name": name, "sequence": {
            "items": vec![json!({"look": 1}); Sequence::MOST]
        }})
    };
    let saved: SavedLook = serde_json::from_value(sequence("loop")).unwrap();
    let excess = serde_json::to_string(&saved).unwrap().len() - 2048;
    let legacy_limit = show_file::MOST_BYTES - LOOKS * (2048 + 6) - 128;
    let padded = short.replacen(
        r#""name": "Red"#,
        &format!(
            r#""name": "Red{}"#,
            "x".repeat(legacy_limit - excess - short.len())
        ),
        1,
    );
    let (patch, _) = show_file::read(&padded, &empty).unwrap();
    drop((sender, first)); // one engine at a time
    // an open takes a front as it starts
    let (_opened, mut sender) = engine::Engine::start(engine::Universes::new(1).unwrap()).unwrap();
    sender
        .open(patch.kept_in(show_file::LIMIT), engine::Looks::new())
        .unwrap();
    let full = Err(engine_core::Refusal::ShowFileFull {
        most: show_file::LIMIT.most as u32,
    });
    for (edit, reply) in [
        (red_step(1, "red"), Ok(())),
        (json!({"PutSequence": sequence("loop")}), Ok(())),
        (json!({"PutSequence": sequence("loops")}), full),
        (json!({"Rename": {"fixture": 1, "name": "PARs"}}), full),
        (json!({"PutSequence": sequence("run")}), Ok(())),
        (json!({"Rename": {"fixture": 1, "name": "PARs"}}), Ok(())),
    ] {
        assert_eq!(
            sender.send(serde_json::from_value(edit).unwrap()),
            reply.map_err(engine::Unsent::Refused)
        );
    }
}

#[tokio::test]
async fn a_step_over_a_patch_that_leaves_the_looks_no_room_is_refused_and_the_file_kept() {
    let file = show_file("no_room");
    let (mut first, listen) = engine(&file, true);
    let mut socket = connect(&listen).await;
    until_show(&mut socket, &json!("Kept")).await;
    assert_eq!(
        patch_once_read(&mut socket, par("PAR 1", 37)).await,
        json!({"Patched": {"fixture": 1}})
    );
    stop(&mut socket, &mut first).await;
    let text = std::fs::read_to_string(&file).unwrap();
    let red = "\"name\": \"Red\"";
    let most = show_file::LIMIT.most;
    let full = json!({"Refused": {"reason": {"ShowFileFull": {"most": most}}}});
    // (the file's length, by the red channel's longer name, what the step gets)
    let cases = [
        (most, json!("Accepted")),
        (most + 1, full.clone()),
        (show_file::MOST_BYTES, full),
    ];
    for (length, reply) in cases {
        let longer = format!("\"name\": \"Red{}\"", "x".repeat(length - text.len()));
        let opened = text.replacen(red, &longer, 1);
        std::fs::write(&file, &opened).unwrap();
        let (mut running, listen) = engine(&file, false);
        let mut socket = connect(&listen).await;
        until_show(&mut socket, &json!("Kept")).await;
        assert_eq!(
            command(&mut socket, red_step(1, "red")).await,
            reply,
            "{length}"
        );
        stop(&mut socket, &mut running).await;
        let saved = std::fs::read_to_string(&file).unwrap();
        if reply == json!("Accepted") {
            assert!(saved.contains("\"red\""), "{length}");
            assert!(saved.len() <= show_file::MOST_BYTES, "{length}");
        } else {
            assert!(saved == opened, "{length}: the file kept");
        }
    }
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn mix_rules_must_fit_before_the_next_frame_or_save() {
    use engine_core::{Brightness, Command, MixRules, Refusal, ShowFile};
    use engine_server::show::{Kept, keep, open};
    use tokio::sync::watch;

    let file = show_file("mix_rules");
    let (mut first, listen) = engine(&file, true);
    let mut socket = connect(&listen).await;
    // A PAR with a dimmer of its own at 40: the brightness setting decides that channel.
    let mut dimmed = par("PAR 1", 37);
    dimmed["Patch"]["mode"] = json!("RGB Dimmer");
    assert_eq!(
        patch_once_read(&mut socket, dimmed).await,
        json!({"Patched": {"fixture": 1}})
    );
    let mut bright = red_step(1, "bright");
    bright["PutStep"]["step"]["values"]["intensity"] = json!(65535);
    assert_eq!(command(&mut socket, bright).await, json!("Accepted"));
    stop(&mut socket, &mut first).await;
    let text = std::fs::read_to_string(&file).unwrap();

    let full = Err(Refusal::ShowFileFull {
        most: show_file::LIMIT.most.try_into().unwrap(),
    });
    // The latest intensity is zero. Highest would turn the dimmer fully on.
    for (length, reply, dimmer) in [(text.len(), Ok(()), 255), (show_file::MOST_BYTES, full, 0)] {
        let name = format!("\"name\": \"Red{}\"", "x".repeat(length - text.len()));
        let opened = text.replacen("\"name\": \"Red\"", &name, 1);
        assert_eq!(opened.len(), length);
        std::fs::write(&file, &opened).unwrap();

        let (_engine, mut sender, wakes) = engine::Engine::start_stepped(
            engine::Universes::new(1).unwrap(),
            Box::new(outputs::open),
        )
        .unwrap();
        assert_eq!(open(&file, &mut sender), ShowFile::Kept);
        let original = Kept::of(&sender);
        wakes.hold().unwrap();

        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let set = json!({"SetOutput": {
            "output": 1, "kind": {"Ddp": {"address": udp.local_addr().unwrap().to_string(), "color": "Rgb"}},
            "universes": {"first": 1, "count": 1}, "rate": null,
        }});
        let start = json!({"StartLook": {"look": 1, "rank": "Manual"}});
        let dark = json!({"SetIntensity": {"target": {"Fixture": {"fixture": 1}}, "intensity": 0}});
        for command in [set, start, dark] {
            sender
                .send(serde_json::from_value(command).unwrap())
                .unwrap();
        }
        wakes.wake().unwrap();
        until_ddp(&udp, &[255, 0, 0, 0]).await;

        let project = sender.project();
        let rules = MixRules {
            brightness: Brightness::Highest,
            ..MixRules::default()
        };
        assert_eq!(
            sender.send(Command::SetMixRules { rules }),
            reply.map_err(engine::Unsent::Refused),
            "{length}"
        );
        assert_eq!(sender.project(), project);
        let (patches, changes) = watch::channel(original.clone());
        assert_eq!(
            patches.send_if_modified(|kept| kept.follow(&sender)),
            reply.is_ok()
        );
        wakes.wake().unwrap();
        until_ddp(&udp, &[255, 0, 0, dimmer]).await;
        wakes.run();

        // Stop flushes the last revision, as it does in the server.
        let (state, shown) = watch::channel(ShowFile::Kept);
        let (_stop, stopped) = watch::channel(true);
        keep(file.clone(), original, changes, state, stopped).await;
        assert_eq!(*shown.borrow(), ShowFile::Kept);
        let saved = std::fs::read_to_string(&file).unwrap();
        if reply.is_ok() {
            assert_eq!(saved, opened.replace("\"Latest\"", "\"Highest\""));
        } else {
            assert!(saved == opened, "the refused rule changed the file");
        }
    }
    std::fs::remove_file(file).unwrap();
}

#[tokio::test]
async fn a_refused_edit_after_a_save_that_failed_leaves_the_file_as_it_was() {
    let file = show_file("refused");
    let (mut running, listen) = engine(&file, true);
    let mut socket = connect(&listen).await;
    until_show(&mut socket, &json!("Kept")).await;
    assert_eq!(
        patch_once_read(&mut socket, par("PAR 1", 37)).await,
        json!({"Patched": {"fixture": 1}})
    );
    assert_eq!(
        command(&mut socket, red_step(1, "one")).await,
        json!("Accepted")
    );
    let start = json!({"StartLook": {"look": 1}});
    assert!(command(&mut socket, start).await.get("Started").is_some());
    until_saved(&file, "\"one\"").await;
    let saved = std::fs::read_to_string(&file).unwrap();

    // the new file cannot be made: a folder has its name
    std::fs::create_dir(saving(&file)).unwrap();
    assert_eq!(
        command(&mut socket, red_step(2, "two")).await,
        json!("Accepted")
    );
    until_show(&mut socket, &json!("NotSaved")).await;
    std::fs::remove_dir(saving(&file)).unwrap();
    // refused edits and a press: none of them saves look 2
    let refused = [
        (
            json!({"RemoveLook": {"look": 1}}),
            json!({"Refused": {"reason": {"LookInUse": {"look": 1}}}}),
        ),
        (
            json!({"Unpatch": {"fixture": 1}}),
            json!({"Refused": {"reason": {"LookInUse": {"look": 1}}}}),
        ),
        (
            json!({"SetChannel": {"channel": {"universe": 1, "address": 1}, "value": 91}}),
            json!("Accepted"),
        ),
    ];
    for (edit, reply) in refused {
        assert_eq!(command(&mut socket, edit.clone()).await, reply, "{edit}");
    }
    // a save would be on disk well within this
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        std::fs::read_to_string(&file).unwrap() == saved,
        "saved by a refusal or a press"
    );
    until_show(&mut socket, &json!("NotSaved")).await;

    // the next accepted edit saves both
    assert_eq!(
        command(&mut socket, red_step(3, "three")).await,
        json!("Accepted")
    );
    until_show(&mut socket, &json!("Kept")).await;
    stop(&mut socket, &mut running).await;
    let text = std::fs::read_to_string(&file).unwrap();
    for name in ["\"one\"", "\"two\"", "\"three\""] {
        assert!(text.contains(name), "{name}: {text}");
    }
    let _ = std::fs::remove_file(&file);
}

/// The test definition of a Generic RGB, read when the test is built.
const GENERIC_RGB: &[u8] =
    include_bytes!("../../fixture-library/tests/definitions/Generic-Generic-RGB.qxf");

#[tokio::test]
async fn an_edit_admitted_before_the_keeper_first_looks_is_saved_on_stop() {
    use engine_server::show::{Kept, keep};
    let file = show_file("first-edit");
    let library = [fixture_library::parse("Generic-Generic-RGB.qxf", GENERIC_RGB).unwrap()];
    let (_engine, mut sender) = engine::Engine::start(engine::Universes::new(1).unwrap()).unwrap();
    // as `serve` starts: the file opened, the keeper made
    let opened = Kept::of(&sender);
    let (patches, _) = tokio::sync::watch::channel(opened.clone());
    let (state, _) = tokio::sync::watch::channel(engine_core::ShowFile::Kept);
    let (stop, _) = tokio::sync::watch::channel(false);
    let keeper = keep(
        file.clone(),
        opened,
        patches.subscribe(),
        state,
        stop.subscribe(),
    );
    // a PAR patched before the keeper first runs, then Stop
    let par = serde_json::from_value(par("PAR", 1)).unwrap();
    sender.begin(par, &library).unwrap();
    patches.send_if_modified(|kept| kept.follow(&sender));
    stop.send_replace(true);
    keeper.await;
    let saved = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(
        saved.contains("\"PAR\""),
        "the first edit is saved: {saved:?}"
    );
    let _ = std::fs::remove_file(&file);
}

#[test]
fn a_show_open_is_atomic_if_admission_runs_out_of_room() {
    use engine::{Engine, Looks, QUEUE, Refusal, Universes};
    use engine_core::{
        ChannelNumbers, Color, Command, PatchCommand, Pixels, ShowFile, Text, Unopened,
    };

    let actual: Vec<_> = [QUEUE - 2, QUEUE - 1]
        .into_iter()
        .map(|filled| {
            let (_engine, mut sender, wakes) = Engine::start_stepped(
                Universes::new(1).unwrap(),
                Box::new(|_| unreachable!("a one-universe patch needs no output")),
            )
            .unwrap();
            wakes.hold().unwrap();
            let at = ChannelNumbers {
                universe: 1,
                address: 1,
            };
            let patch = sender
                .patch()
                .update(
                    PatchCommand::PatchStrip {
                        name: Text::new("Strip").unwrap(),
                        pixels: Pixels::try_from(1).unwrap(),
                        color: Color::Rgb,
                        at,
                    },
                    &[],
                )
                .unwrap()
                .0
                .unwrap();
            let path = show_file("review-atomic");
            std::fs::write(&path, show_file::write(&patch, &Looks::new()).unwrap()).unwrap();
            for _ in 0..filled {
                sender
                    .send(Command::SetChannel {
                        channel: at,
                        value: 7,
                    })
                    .unwrap();
            }
            let result = engine_server::show::open(&path, &mut sender);
            std::fs::remove_file(&path).unwrap();
            (result, sender.patch().fixtures().len())
        })
        .collect();
    assert_eq!(
        actual,
        [
            (ShowFile::Kept, 1),
            (
                ShowFile::LeftAlone {
                    why: Unopened::Refused {
                        reason: Refusal::QueueFull
                    },
                },
                0,
            ),
        ]
    );
}

/// What a show file holds, for the rows of the open.
#[derive(Clone, Copy)]
enum Holds {
    NotThere,
    AFolder,
    NotJson,
    /// A strip from 1:510 into universe 2: no output streams them at start.
    AcrossTwoUniverses,
    /// A dimmer, look 1 on it, and Mixer settings other than the defaults.
    ADimmerAndALook,
}

/// Every row of the open (`docs/engine.md`, Show file): all or nothing. Each
/// `LeftAlone` row leaves the patch, the looks and the Mixer's settings as they start;
/// only `Kept` takes them, together. The open takes 2 places of the queue (writers and
/// settings in one chunk).
#[test]
fn a_show_opens_whole_or_not_at_all() {
    use engine::{Command, Engine, Looks, QUEUE, Refusal, Universes};
    use engine_core::{
        Brightness, ChannelNumbers, Color, ManualEnd, MixRules, OtherEnd, PatchCommand, Pixels,
        ShowFile, Text, Unopened,
    };

    let dimmer = format!(
        r#"{{
          "version": {},
          "patch": [{{"fixture":1,"name":"Dimmer","mode":"m","at":{{"universe":1,"address":1}},"first":1}}],
          "groups": [],
          "looks": [{{"look":1,"name":"Full","step":{{"target":{{"Fixture":{{"fixture":1}}}},"values":{{"intensity":65535}}}}}}],
          "rules": {{"brightness":"Highest","manual_end":"UntilStop","other_end":"Stay"}},
          "modes": {{"m":{{"maker":"Example","model":"Dimmer","mode":{{
            "name":"1ch","channels":[{{"name":"Dimmer","function":"Dimmer","byte":"Coarse","default":null,"direction":null,"ranges":[]}}],
            "pixels":[],"layout":null,"degrees":null
          }}}}}}
        }}"#,
        show_file::VERSION
    );
    let set = MixRules {
        brightness: Brightness::Highest,
        manual_end: ManualEnd::UntilStop,
        other_end: OtherEnd::Stay,
    };
    let unset = MixRules::default();
    let left = |reason| ShowFile::LeftAlone {
        why: Unopened::Refused { reason },
    };
    // (case, what the file holds, queue places taken before the open, the answer,
    // fixtures, looks and settings after)
    let cases = [
        ("not there", Holds::NotThere, 0, ShowFile::Kept, 0, 0, unset),
        (
            "not there, no room",
            Holds::NotThere,
            QUEUE - 1,
            left(Refusal::QueueFull),
            0,
            0,
            unset,
        ),
        (
            "a folder",
            Holds::AFolder,
            0,
            ShowFile::LeftAlone {
                why: Unopened::CannotRead,
            },
            0,
            0,
            unset,
        ),
        (
            "not JSON",
            Holds::NotJson,
            0,
            ShowFile::LeftAlone {
                why: Unopened::Broken { line: 1, column: 1 },
            },
            0,
            0,
            unset,
        ),
        (
            "a strip no output streams",
            Holds::AcrossTwoUniverses,
            0,
            left(Refusal::NotStreamed { first: 1, last: 2 }),
            0,
            0,
            unset,
        ),
        (
            "a dimmer and a look",
            Holds::ADimmerAndALook,
            QUEUE - 2,
            ShowFile::Kept,
            1,
            1,
            set,
        ),
        (
            "a dimmer and a look, no room",
            Holds::ADimmerAndALook,
            QUEUE - 1,
            left(Refusal::QueueFull),
            0,
            0,
            unset,
        ),
    ];
    for (case, holds, taken, answer, fixtures, looks, rules) in cases {
        let (_engine, mut sender, wakes) = Engine::start_stepped(
            Universes::new(2).unwrap(),
            Box::new(|_| unreachable!("the open opens no output")),
        )
        .unwrap();
        wakes.hold().unwrap();
        let path = show_file("open-rows");
        match holds {
            Holds::NotThere => {}
            Holds::AFolder => std::fs::create_dir(&path).unwrap(),
            Holds::NotJson => std::fs::write(&path, "x").unwrap(),
            Holds::AcrossTwoUniverses => {
                let strip = PatchCommand::PatchStrip {
                    name: Text::new("Across").unwrap(),
                    pixels: Pixels::try_from(2).unwrap(),
                    color: Color::Rgb,
                    at: ChannelNumbers {
                        universe: 1,
                        address: 510,
                    },
                };
                let patch = sender.patch().update(strip, &[]).unwrap().0.unwrap();
                let text = show_file::write(&patch, &Looks::new()).unwrap();
                std::fs::write(&path, text).unwrap();
            }
            Holds::ADimmerAndALook => std::fs::write(&path, &dimmer).unwrap(),
        }
        for _ in 0..taken {
            sender.send(Command::Blackout { on: false }).unwrap();
        }
        let opened = engine_server::show::open(&path, &mut sender);
        if path.is_dir() {
            std::fs::remove_dir(&path).unwrap();
        } else if path.exists() {
            std::fs::remove_file(&path).unwrap();
        }
        let now = sender.looks();
        assert_eq!(
            (
                opened,
                sender.patch().fixtures().len(),
                now.rows().count(),
                now.rules()
            ),
            (answer, fixtures, looks, rules),
            "{case}"
        );
    }
}
