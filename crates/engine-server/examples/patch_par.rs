//! Patches a Generic RGB PAR (mode RGB: red, green, blue) at universe 1, `address`, makes
//! it red, green, blue, 3 seconds each, then unpatches it: dark. A check by eye on a real
//! device.
//!
//! | Device | Command | Look at |
//! |---|---|---|
//! | WLED over DDP | `cargo run -p engine-server --example patch_par -- ddp 192.168.1.50` | pixel 13 (channels 37 to 39) |
//! | DMX USB Pro | `cargo run -p engine-server --example patch_par -- dmx 1` | an RGB PAR at address 37 |
//!
//! An address after the device patches it there instead of 37: `-- dmx 1 101`.
//!
//! It drives the engine like any client: `SetOutput`, `Patch`, `SetIntensity`,
//! `SetColour`, `Unpatch`.

use engine::{
    Adapter, Begun, ChannelNumbers, Color, Command, Engine, Fixture, OutputKind, OutputNumber,
    RunNumbers, Sender, Target, Text, Universes,
};
use std::net::SocketAddrV4;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

const USAGE: &str = "usage: patch_par (ddp <ip>[:port] | dmx <adapter>) [address]";

/// The test definition of a Generic RGB, read when the example is built.
const GENERIC_RGB: &[u8] =
    include_bytes!("../../fixture-library/tests/definitions/Generic-Generic-RGB.qxf");

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("patch_par: {problem}\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let kind = match (args.first().map(String::as_str), args.get(1)) {
        (Some("ddp"), Some(target)) => OutputKind::Ddp {
            address: target
                .parse::<SocketAddrV4>()
                .or_else(|_| format!("{target}:4048").parse())
                .map_err(|_| format!("not an IPv4 address: {target}"))?,
            color: Color::Rgb,
        },
        (Some("dmx"), Some(adapter)) => OutputKind::DmxUsbPro {
            adapter: adapter
                .parse::<u8>()
                .ok()
                .and_then(|n| Adapter::try_from(n).ok())
                .ok_or_else(|| format!("not an adapter number: {adapter}"))?,
        },
        _ => return Err("which device?".into()),
    };
    let address = args
        .get(2)
        .map_or(Ok(37), |a| a.parse::<u16>())
        .map_err(|_| "address")?;
    let library = vec![
        fixture_library::parse("Generic-Generic-RGB.qxf", GENERIC_RGB)
            .map_err(|e| format!("{e:?}"))?,
    ];

    let (mut engine, mut sender) = Engine::start(Universes::ONE).map_err(|e| e.to_string())?;
    let output = OutputNumber::try_from(1).map_err(|_| "output")?;
    let add = Command::SetOutput {
        output,
        kind,
        universes: RunNumbers { first: 1, count: 1 },
        rate: None,
    };
    send(&mut sender, add, &library)?;
    let text = |text| Text::new(text).ok_or("a short text");
    let patch = Command::Patch {
        name: text("PAR 3")?,
        maker: text("Generic")?,
        model: text("Generic RGB")?,
        mode: text("RGB")?,
        at: ChannelNumbers {
            universe: 1,
            address,
        },
    };
    let fixture = send(&mut sender, patch, &library)?.ok_or("no fixture number")?;
    let target = Target::Fixture { fixture };
    let full = Command::SetIntensity {
        target,
        intensity: u16::MAX,
    };
    send(&mut sender, full, &library)?;
    let colours = [
        ("red", [u16::MAX, 0, 0]),
        ("green", [0, u16::MAX, 0]),
        ("blue", [0, 0, u16::MAX]),
    ];
    for (name, [red, green, blue]) in colours {
        let colour = Command::SetColour {
            target,
            red,
            green,
            blue,
        };
        send(&mut sender, colour, &library)?;
        println!("\x07fixture {fixture} at 1.{address}: {name} now");
        thread::sleep(Duration::from_secs(3));
    }
    send(&mut sender, Command::Unpatch { fixture }, &library)?;
    println!("\x07unpatched: dark now");
    thread::sleep(Duration::from_secs(2));
    let state = engine.fresh().state;
    if let Some(output) = state.outputs.first().copied().flatten() {
        println!("output 1: {:?}, {:?}", output.status, output.counters);
    }
    Ok(())
}

/// Sends `command` as any client does. The fixture number of a patch.
fn send(sender: &mut Sender, command: Command, library: &[Fixture]) -> Result<Option<u16>, String> {
    match sender.begin(command, library) {
        Ok(Begun::Queued | Begun::Started { .. }) => Ok(None),
        Ok(Begun::Patched { fixture }) => Ok(Some(fixture)),
        Ok(Begun::Opening(opening)) => sender
            .finish(opening.open())
            .map(|()| None)
            .map_err(|r| format!("{command:?}: refused: {r:?}")),
        Err(refused) => Err(format!("{command:?}: refused: {refused:?}")),
    }
}
