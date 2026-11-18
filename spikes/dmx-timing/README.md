# Spike: glitch-free USB DMX on macOS (#23)

Throwaway. Findings and numbers: [docs/spikes/dmx-timing.md](../../docs/spikes/dmx-timing.md).
Outside the Cargo workspace and outside `npm test`. Run from this folder.

`PRO=$(ls /dev/cu.usbserial-* | head -1)` finds the adapter. `RT="--priority realtime --wait spin"` is the setup that holds 44 Hz.

| Scenario | Mock | DMX USB Pro | DDP (WLED) |
|---|---|---|---|
| Idle | `cargo run --release -- $RT` | `cargo run --release -- --sink pro=$PRO $RT` | `cargo run --release -- --sink ddp=HOST:4048 --api HOST:PORT $RT` |
| Heavy CPU | add `--load 10` | run next to the mock with load | run next to the mock with load |
| Background (App Nap stand-in) | `taskpolicy -b ./target/release/dmx-timing $RT` | the same with `--sink pro=$PRO` | the same with `--sink ddp=...` |
| Unplug and replug | | `cargo run --release -- --sink pro=$PRO $RT --hands unplug` | |
| Sleep and wake | | `cargo run --release -- --sink pro=$PRO $RT --hands sleep` | |
| Thread setups (2 min each) | `cargo run --release -- --priority default\|qos\|realtime --wait sleep\|spin --seconds 120` | | |
| Open DMX (not run: no adapter) | | `cargo run --release -- --sink open-dmx=$PORT $RT` | |

- Every run lasts 10 minutes unless `--seconds` says otherwise.
- `--load N`: N threads busy on arithmetic and memory meanwhile (10 = the cores of the test machine).
- `--hands`: prints, with a bell, when to unplug and plug in, or when to sleep and wake.
- `--api`: the WLED HTTP API, checked with `/json/info` before the run. DDP defaults to `127.0.0.1:4048`.
- `--json FILE`: the statistics as JSON, as in `results/`. It names the sink kind, never the port.
- The serial sinks are macOS only. The port is opened exclusively and closed at the end.
