//! The engine process: one per machine and port, and an orderly stop over the API.

use futures_util::{SinkExt, StreamExt};
use std::io::{BufRead, BufReader, Read};
use std::net::TcpListener;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::Message;

/// A port nobody listens on right now.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// An engine process, killed when the test ends however it ends.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn engine(listen: &str) -> Running {
    Running(
        Command::new(env!("CARGO_BIN_EXE_bossylights-engine"))
            .args(["--listen", listen])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    )
}

/// Waits for the "listening" line, so the engine holds its port.
fn listening(Running(engine): &mut Running) {
    let mut line = String::new();
    BufReader::new(engine.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.contains("listening"), "{line}");
}

/// The exit status, waiting at most 10 s.
fn exited(Running(engine): &mut Running) -> ExitStatus {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = engine.try_wait().unwrap() {
            return status;
        }
        assert!(Instant::now() < until, "the engine did not end within 10 s");
        thread::sleep(Duration::from_millis(50)); // poll
    }
}

fn stderr(Running(engine): &mut Running) -> String {
    let mut text = String::new();
    engine
        .stderr
        .as_mut()
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

/// Sends `Stop` and returns the answer.
async fn stop(listen: &str) -> serde_json::Value {
    let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{listen}/api"))
        .await
        .unwrap();
    socket.send(Message::text("\"Stop\"")).await.unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("an answer within 2 s")
        .unwrap()
        .unwrap();
    serde_json::from_str(reply.to_text().unwrap()).unwrap()
}

#[tokio::test]
async fn a_second_engine_on_the_same_port_refuses_to_start_and_says_why() {
    // (first listens on, second listens on): the same port, any address
    let cases = [("127.0.0.1", "127.0.0.1"), ("127.0.0.1", "[::1]")];
    for (first, second) in cases {
        let port = free_port();
        let (first, second) = (format!("{first}:{port}"), format!("{second}:{port}"));
        let mut running = engine(&first);
        listening(&mut running);

        let mut refused = engine(&second);
        let status = exited(&mut refused);
        let reason = stderr(&mut refused);
        assert_eq!(status.code(), Some(1), "{second}: {reason}");
        assert!(
            reason.contains(&format!("another engine already runs on port {port}")),
            "{second}: {reason}"
        );

        assert_eq!(
            running.0.try_wait().unwrap(),
            None,
            "the first one still runs"
        );
        assert_eq!(stop(&first).await, serde_json::json!("Stopping"));
        exited(&mut running);
    }
}

#[tokio::test]
async fn stop_ends_the_process_reports_what_it_measured_and_frees_the_port() {
    let listen = format!("127.0.0.1:{}", free_port());
    let mut first = engine(&listen);
    listening(&mut first);

    assert_eq!(stop(&listen).await, serde_json::json!("Stopping"));
    let status = exited(&mut first);
    let report = stderr(&mut first);
    assert!(status.success(), "{report}");
    assert!(report.contains("stopped at frame "), "{report}");

    let mut next = engine(&listen);
    listening(&mut next);
    assert_eq!(stop(&listen).await, serde_json::json!("Stopping"));
    assert!(exited(&mut next).success());
}
