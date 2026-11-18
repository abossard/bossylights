import { createSignal, For, onCleanup, Show } from "solid-js";
import { store, type ClockFeed, type Mapping, type Output } from "../../fake/store";
import { Button, Decks, Meter, Region, Table } from "../../kit";
import type { Issue } from "../../shell/registry";
import "./inout.css";
import { AddOutput, AudioInput, Osc, OutputSetup } from "./Setup";

export const id = "inout";
export const title = "In/Out";
export const order = 6;
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

const DEVICES = ["Launchpad Mini", "APC mini"];

const STATE: Record<Output["state"], string> = { connected: "Sending", reconnecting: "Reconnecting", error: "Error" };

export function Screen() {
  // ---- mappings: what Learn made, to edit, test and remove
  const [editing, setEditing] = createSignal<string | null>(null);
  const [tested, setTested] = createSignal<string | null>(null);
  let mapTimer: number | undefined;
  const testMapping = (m: Mapping) => {
    setTested(m.id);
    clearTimeout(mapTimer);
    mapTimer = window.setTimeout(() => setTested(null), 2000);
  };
  onCleanup(() => clearTimeout(mapTimer));
  const testedMapping = () => store.state.mappings.find((m) => m.id === tested());

  // ---- audio level, moving with the beat
  const level = () => {
    const p = store.beatPhase();
    const t = performance.now() / 1000;
    return Math.min(1, 0.3 + 0.55 * (1 - p) ** 2 + 0.08 * Math.sin(t * 13) * Math.sin(t * 7));
  };

  // ---- OSC: a message now and then
  const oscAgo = () => Math.floor((store.now() * 10) % 40) / 10;

  const tapped = () => (store.state.clockFeed === "tap" ? store.state.tempo : null);

  const FEEDS: { id: ClockFeed; name: string; tempo: () => number | null; sure: number; note: () => string }[] = [
    { id: "tap", name: "Tap", tempo: () => tapped(), sure: 0.6, note: () => (tapped() ? "From your taps" : "Tap here or in the top bar") },
    { id: "audio", name: "Audio", tempo: () => 125.8, sure: 0.82, note: () => "From the audio input" },
    { id: "midi", name: "MIDI clock", tempo: () => null, sure: 0, note: () => "No clock coming in" },
    { id: "vdj", name: "VirtualDJ", tempo: () => 126, sure: 0.99, note: () => "Deck 1, playing" },
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

        <section class="io-sec">
          <h3 class="io-h">Audio input</h3>
          <div class="io-row">
            <span class="io-text">
              <b>Built-in input</b>
              <span class="mute">Beat found at 125.8 BPM</span>
            </span>
            <Meter value={level()} label="Audio level" class="io-meter" />
          </div>
          <AudioInput />
        </section>
        <Osc />

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
        <Button size="lg" class="io-tap" onClick={() => store.tap()}>
          Tap
        </Button>
        <p class="mute">Tap along with the music. After two taps the beat follows you.</p>
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
                <OutputSetup name={o.device} live={store.state.night.live} />
              </li>
            )}
          </For>
        </ul>
        <AddOutput />
      </Region>

      <Region
        id="mappings"
        title="Mappings"
        class="io-map"
        aside={<span class="mute">Made with Learn in the top bar: press it, click any control on any screen, then move a control on your hardware</span>}
      >
        <div class="io-map-scroll">
          <Table<Mapping>
            caption="Controls that hardware can reach"
            rows={store.state.mappings}
            rowKey={(m) => m.id}
            selected={tested()}
            empty="Nothing mapped yet. Press Learn in the top bar."
            columns={[
              { key: "control", title: "Control", render: (m) => <b>{m.control}</b> },
              { key: "screen", title: "Screen" },
              {
                key: "device",
                title: "Device",
                render: (m) => (
                  <Show when={editing() === m.id} fallback={m.device}>
                    <select aria-label={`Device for ${m.control}`} value={m.device} onChange={(e) => store.updateMapping(m.id, { device: e.currentTarget.value })}>
                      <For each={DEVICES}>{(d) => <option value={d}>{d}</option>}</For>
                    </select>
                  </Show>
                ),
              },
              {
                key: "message",
                title: "Note or controller number",
                render: (m) => (
                  <Show when={editing() === m.id} fallback={m.message}>
                    <input
                      type="text"
                      class="io-map-input"
                      aria-label={`Note or controller number for ${m.control}`}
                      value={m.message}
                      onChange={(e) => store.updateMapping(m.id, { message: e.currentTarget.value.trim() || m.message })}
                    />
                  </Show>
                ),
              },
              {
                key: "feedback",
                title: "Feedback",
                render: (m) => (
                  <Show when={editing() === m.id} fallback={m.feedback ? "Lights up" : "None"}>
                    <Button pressed={m.feedback} onClick={() => store.updateMapping(m.id, { feedback: !m.feedback })}>
                      {m.feedback ? "Lights up" : "None"}
                    </Button>
                  </Show>
                ),
              },
              {
                key: "actions",
                title: "",
                align: "right",
                render: (m) => (
                  <span class="io-map-actions">
                    <Button pressed={editing() === m.id} aria-label={`${editing() === m.id ? "Done editing" : "Edit"} ${m.control}`} onClick={() => setEditing(editing() === m.id ? null : m.id)}>
                      {editing() === m.id ? "Done" : "Edit"}
                    </Button>
                    <Button pressed={tested() === m.id} aria-label={`Test ${m.control}`} onClick={() => testMapping(m)}>
                      {tested() === m.id ? "Sent" : "Test"}
                    </Button>
                    <Button variant="quiet" aria-label={`Remove ${m.control}`} onClick={() => store.removeMapping(m.id)}>
                      Remove
                    </Button>
                  </span>
                ),
              },
            ]}
          />
        </div>
        <p class="io-map-msg" role="status">
          <Show when={testedMapping()}>
            {(m) => (
              <>
                Test: {m().device} sent {m().message}, {m().control} answered{m().feedback ? " and the control lit up" : ""}.
              </>
            )}
          </Show>
        </p>
      </Region>
    </div>
  );
}
