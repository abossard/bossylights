import { createSignal, For, onCleanup, Show } from "solid-js";
import { createStore } from "solid-js/store";
import { store, type ClockFeed, type Output } from "../../fake/store";
import { Button, Decks, Meter, Region } from "../../kit";
import type { Issue } from "../../shell/registry";
import "./inout.css";

export const id = "inout";
export const title = "In/Out";
export const order = 7;
export const issues: Issue[] = [
  { number: 42, title: "USB DMX adapters" },
  { number: 43, title: "Art-Net and sACN" },
  { number: 44, title: "DDP pixel output" },
  { number: 33, title: "MIDI in with learn" },
  { number: 92, title: "Ready-made controller profiles" },
  { number: 34, title: "OSC in and out" },
  { number: 35, title: "Audio in: beat and levels" },
  { number: 45, title: "One tempo for everything" },
];

/** What a controller press can do. */
const ACTIONS = [
  { id: "blackout", name: "Blackout", run: () => store.setBlackout(!store.state.blackout) },
  { id: "surprise", name: "Surprise me", run: () => store.surprise() },
  { id: "release", name: "Release Manual", run: () => store.releaseAll() },
  { id: "undo", name: "Undo", run: () => store.undo() },
];

// Learned mappings outlive the screen, like a real controller profile would.
const [map, setMap] = createStore<Record<string, string>>({ blackout: "8,4" });

const STATE: Record<Output["state"], string> = { connected: "Sending", reconnecting: "Reconnecting", error: "Error" };

export function Screen() {
  // ---- MIDI learn: step 1 pick what to learn, step 2 press on the controller
  const [learning, setLearning] = createSignal<string | null>(null);
  const [lastPress, setLastPress] = createSignal("");
  const padName = (k: string) => `pad ${k.replace(",", " in row ")}`;
  const press = (key: string) => {
    const l = learning();
    if (l) {
      const before = { ...map };
      const old = Object.keys(map).find((a) => map[a] === key);
      setMap((m) => ({ ...m, ...(old ? { [old]: undefined as unknown as string } : {}), [l]: key }));
      store.record(`Learn ${ACTIONS.find((a) => a.id === l)!.name}`, () => setMap(() => before));
      setLearning(null);
      setLastPress(`Learned: ${padName(key)} now does ${ACTIONS.find((a) => a.id === l)!.name}.`);
      return;
    }
    const action = ACTIONS.find((a) => map[a.id] === key);
    if (action) {
      action.run();
      setLastPress(`${padName(key)}: ${action.name}`);
    } else setLastPress(`${padName(key)}: nothing on it yet. Use Learn to give it a job.`);
  };
  const onKey = (e: KeyboardEvent) => e.key === "Escape" && setLearning(null);
  window.addEventListener("keydown", onKey);
  onCleanup(() => window.removeEventListener("keydown", onKey));

  // ---- audio level, moving with the beat
  const level = () => {
    const p = store.beatPhase();
    const t = performance.now() / 1000;
    return Math.min(1, 0.3 + 0.55 * (1 - p) ** 2 + 0.08 * Math.sin(t * 13) * Math.sin(t * 7));
  };

  // ---- OSC: a message now and then
  const oscAgo = () => Math.floor((store.now() * 10) % 40) / 10;

  // ---- tap tempo
  const [taps, setTaps] = createSignal<number[]>([]);
  const tapBpm = () => {
    const t = taps();
    if (t.length < 2) return null;
    const gaps = t.slice(1).map((x, i) => x - t[i]);
    return 60000 / (gaps.reduce((a, b) => a + b, 0) / gaps.length);
  };
  const tap = () => {
    const now = performance.now();
    const t = [...taps().filter((x) => now - x < 3000), now].slice(-5);
    setTaps(t);
    const b = tapBpm();
    if (b && store.state.clockFeed === "tap") store.setTempo(b);
  };

  const FEEDS: { id: ClockFeed; name: string; tempo: () => number | null; sure: number; note: () => string }[] = [
    { id: "tap", name: "Tap", tempo: () => tapBpm(), sure: 0.6, note: () => (tapBpm() ? `${taps().length} taps` : "Tap the button four times") },
    { id: "audio", name: "Audio", tempo: () => 125.8, sure: 0.82, note: () => "From the audio input" },
    { id: "midi", name: "MIDI clock", tempo: () => null, sure: 0, note: () => "No clock coming in" },
    { id: "vdj", name: "VirtualDJ", tempo: () => 126, sure: 0.99, note: () => "Deck A, on air" },
    { id: "timecode", name: "Timecode", tempo: () => null, sure: 0, note: () => "No timecode coming in" },
  ];

  // ---- output test
  const [testing, setTesting] = createSignal<string | null>(null);
  let testTimer: number | undefined;
  const test = (o: Output) => {
    setTesting(o.id);
    clearTimeout(testTimer);
    testTimer = window.setTimeout(() => setTesting(null), 3000);
  };
  onCleanup(() => clearTimeout(testTimer));

  return (
    <div class="io-screen">
      <Region id="in" title="In" class="io-col">
        <section class="io-sec">
          <h3 class="io-h">Controllers</h3>
          <ul class="io-rows">
            <li class="io-row">
              <i class="io-lamp is-on" aria-hidden="true" />
              <span class="io-text">
                <b>Launchpad Mini</b>
                <span class="mute">Ready-made profile · pads mirror the Console</span>
              </span>
              <span class="io-state">Connected</span>
            </li>
            <li class="io-row is-missing">
              <i class="io-lamp" aria-hidden="true" />
              <span class="io-text">
                <b>APC mini</b>
                <span class="mute">Ready-made profile · plug it in to use it</span>
              </span>
              <span class="io-state">Not found</span>
            </li>
          </ul>
        </section>

        <section class="io-sec" aria-label="MIDI learn">
          <h3 class="io-h">Give a pad a job</h3>
          <ul class="io-rows">
            <For each={ACTIONS}>
              {(a) => (
                <li class="io-row io-learn-row" classList={{ "is-learning": learning() === a.id }}>
                  <span class="io-text">
                    <b>{a.name}</b>
                    <span class="mute">{map[a.id] ? `Launchpad Mini, ${padName(map[a.id])}` : "Not on a pad"}</span>
                  </span>
                  <Button pressed={learning() === a.id} onClick={() => setLearning(learning() === a.id ? null : a.id)}>
                    {learning() === a.id ? "Cancel" : "Learn"}
                  </Button>
                </li>
              )}
            </For>
          </ul>
          <p class="io-learn-msg" classList={{ "is-waiting": !!learning() }} role="status">
            {learning()
              ? `Waiting: press a pad on the Launchpad for ${ACTIONS.find((a) => a.id === learning())!.name}.`
              : lastPress() || "Press Learn, then a pad on the controller."}
          </p>
          <div class="io-pads" classList={{ "is-waiting": !!learning() }} role="group" aria-label="Launchpad Mini, as it is now">
            <For each={[1, 2, 3, 4]}>
              {(row) => (
                <For each={[1, 2, 3, 4, 5, 6, 7, 8]}>
                  {(col) => {
                    const key = `${col},${row}`;
                    const job = () => ACTIONS.find((a) => map[a.id] === key);
                    return (
                      <button
                        type="button"
                        class="io-pad"
                        classList={{ "has-job": !!job() }}
                        aria-label={`Pad ${col} in row ${row}${job() ? `: ${job()!.name}` : ""}`}
                        onClick={() => press(key)}
                      />
                    );
                  }}
                </For>
              )}
            </For>
          </div>
        </section>

        <section class="io-sec">
          <h3 class="io-h">Audio input</h3>
          <div class="io-row">
            <span class="io-text">
              <b>Built-in input</b>
              <span class="mute">Beat found at 125.8 BPM</span>
            </span>
            <Meter value={level()} label="Audio level" class="io-meter" />
          </div>
        </section>

        <section class="io-sec">
          <h3 class="io-h">VirtualDJ</h3>
          <div class="io-row">
            <i class="io-lamp is-on" aria-hidden="true" />
            <span class="io-text">
              <b>Connected</b>
              <span class="mute">On this laptop, plugin 1.2</span>
            </span>
          </div>
          <Decks decks={store.decks()} compact class="io-decks" />
        </section>

        <section class="io-sec">
          <h3 class="io-h">OSC</h3>
          <div class="io-row">
            <i class="io-lamp is-on" aria-hidden="true" />
            <span class="io-text">
              <b>Listening on port 8000</b>
              <span class="mute">
                Last message {oscAgo().toFixed(1)} s ago: /master 0.85
              </span>
            </span>
          </div>
        </section>
      </Region>

      <Region id="clock" title="Clock" class="io-col" aside={<span class="mute">Where the beat comes from</span>}>
        <div class="io-tempo">
          <b class="display">{store.state.tempo}</b>
          <span class="mute">BPM, from {FEEDS.find((f) => f.id === store.state.clockFeed)!.name}</span>
        </div>
        <div class="io-feeds" role="radiogroup" aria-label="Clock source">
          <For each={FEEDS}>
            {(f) => (
              <button
                type="button"
                role="radio"
                class="io-feed"
                classList={{ "no-signal": f.tempo() === null }}
                aria-checked={store.state.clockFeed === f.id}
                onClick={() => {
                  store.setClockFeed(f.id);
                  const t = f.tempo();
                  if (t) store.setTempo(t);
                }}
              >
                <span class="io-feed-name">
                  <b>{f.name}</b>
                  <span>{f.note()}</span>
                </span>
                <span class="io-feed-tempo">
                  <b class="display">{f.tempo() ? f.tempo()!.toFixed(1) : "none"}</b>
                  <Show when={f.tempo()}>
                    <span class="io-sure">
                      <Meter value={f.sure} label={`${f.name}: how sure`} />
                      {Math.round(f.sure * 100)}% sure
                    </span>
                  </Show>
                </span>
              </button>
            )}
          </For>
        </div>
        <Button size="lg" class="io-tap" onClick={tap}>
          Tap
        </Button>
        <p class="mute">Tap along with the music. Pick Tap above to make it the beat.</p>
      </Region>

      <Region id="out" title="Out" class="io-col" aside={<span class="mute">Where the lights get their data</span>}>
        <ul class="io-outs">
          <For each={store.outputs()}>
            {(o) => (
              <li class={`io-out is-${o.state}`}>
                <header>
                  <span class="io-kind">{o.kind}</span>
                  <span class={`io-out-state is-${o.state}`}>{STATE[o.state]}</span>
                </header>
                <b>{o.device}</b>
                <span class="mute">
                  {o.universe ? `Universe ${o.universe}` : "Pixels"} · {o.fps} frames per second · {o.errors} errors
                </span>
                <span class="io-detail">{testing() === o.id ? "Testing: every channel at half for 3 seconds" : o.detail}</span>
                <Button
                  pressed={testing() === o.id}
                  disabled={o.state === "error"}
                  reason="Fix the connection first"
                  onClick={() => test(o)}
                >
                  {testing() === o.id ? "Testing" : "Test"}
                </Button>
              </li>
            )}
          </For>
        </ul>
      </Region>
    </div>
  );
}
