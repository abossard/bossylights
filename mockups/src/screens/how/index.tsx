import { createSignal, For, onCleanup, Show } from "solid-js";
import { lookById, store } from "../../fake/store";
import { Button, Fader, Pad, Region, Segmented, type PadState } from "../../kit";
import type { Issue } from "../../shell/registry";
import { createDots, Flow } from "./Flow";
import "./how.css";

export const id = "how";
export const title = "How it works";
export const order = 7;
export const issues: Issue[] = [
  { number: 100, title: "Review the theory and the module boundary map" },
  { number: 72, title: "Layers: base, override, flash" },
  { number: 94, title: "Record what you busk" },
];

const PADS = ["strobe", "sunset", "fan-out", "chase-lr"];
const HOLD_MS = 350;
const ROUTE_YOU = ["p-you", "p-manual", "p-frame", "p-rig"];
const ROUTE_SHOW = ["p-showw", "p-show", "p-frame", "p-rig"];
const ROUTE_AUTO = ["p-autow", "p-auto", "p-frame", "p-rig"];

interface Step {
  say: string;
  lit: string[];
  route?: string[];
}

const WALKS: { id: string; name: string; steps: Step[] }[] = [
  {
    id: "press",
    name: "Press a pad",
    steps: [
      { say: "You press Sunset on the screen.", lit: ["you"] },
      { say: "One command goes out: start Sunset on Manual.", lit: ["you", "p-you", "manual"], route: ["p-you"] },
      { say: "The next frame includes Sunset.", lit: ["manual", "p-manual", "frame"], route: ["p-manual"] },
      { say: "The rig turns the frame into DMX for the lights.", lit: ["frame", "p-frame", "rig", "p-rig", "output"], route: ["p-frame", "p-rig"] },
      { say: "Screen and Launchpad light up. Nobody told the Launchpad.", lit: ["you", "controller", "manual"] },
    ],
  },
  {
    id: "record",
    name: "Record and replay",
    steps: [
      { say: "Recording on: your commands are written down with their time.", lit: ["you", "p-you", "manual"], route: ["p-you"] },
      { say: "That list is now a show.", lit: ["showw"] },
      { say: "The show replays the same commands on the Show layer.", lit: ["showw", "p-showw", "show", "p-show", "frame"], route: ["p-showw", "p-show"] },
      { say: "Grab a fader: Manual is on top, you win.", lit: ["you", "p-you", "manual", "p-manual", "frame"], route: ["p-you", "p-manual"] },
      { say: "Let go: the show is back.", lit: ["show", "p-show", "frame"], route: ["p-show"] },
    ],
  },
  {
    id: "yield",
    name: "The autopilot yields",
    steps: [
      { say: "The autopilot plays a rising look on every light.", lit: ["autow", "p-autow", "auto", "p-auto", "frame"], route: ["p-autow", "p-auto"] },
      { say: "You hold Strobe on the PARs.", lit: ["you", "p-you", "manual"], route: ["p-you"] },
      { say: "PARs follow you, movers follow the autopilot.", lit: ["manual", "auto", "p-manual", "p-auto", "frame"] },
      { say: "Release: the PARs go back to the autopilot, no cut.", lit: ["auto", "p-auto", "frame"], route: ["p-auto"] },
    ],
  },
];

interface Chip {
  lookId: string;
  action: "start" | "stop" | "flash";
  at: number;
  dur?: number;
}

const LAYER_NAME = { manual: "Manual", show: "Show", autopilot: "Autopilot" } as const;

export function Screen() {
  const dots = createDots();

  // ---- the walks
  const [walk, setWalk] = createSignal(WALKS[0].id);
  const [step, setStep] = createSignal(-1);
  const steps = () => WALKS.find((w) => w.id === walk())!.steps;
  const current = () => (step() >= 0 ? steps()[step()] : null);
  const goStep = (i: number) => {
    setStep(i);
    const r = steps()[i]?.route;
    if (r) dots.send(r);
  };

  // ---- pads: tap starts or stops, hold flashes
  const wasRunning = new Set<string>();
  const downAt: Record<string, number> = {};
  const [recording, setRecording] = createSignal(false);
  const [chips, setChips] = createSignal<Chip[]>([]);
  const write = (c: Chip) => recording() && setChips([...chips(), c]);

  const down = (id: string) => {
    dots.send(ROUTE_YOU);
    downAt[id] = performance.now();
    const s = store.lookState(id);
    if (s === "held" || s === "flash") wasRunning.add(id);
    else store.hold(id);
  };
  const up = (id: string, ms: number) => {
    if (wasRunning.delete(id)) {
      if (ms < HOLD_MS) {
        store.stop(id);
        write({ lookId: id, action: "stop", at: performance.now() });
      }
    } else if (ms < HOLD_MS) {
      store.start(id);
      write({ lookId: id, action: "start", at: downAt[id] });
    } else {
      store.release(id);
      write({ lookId: id, action: "flash", at: downAt[id], dur: ms });
    }
  };
  const padState = (id: string): PadState => store.lookState(id) ?? "idle";

  // ---- replay on the Show layer
  const [replaying, setReplaying] = createSignal(false);
  const timers: number[] = [];
  const stopReplay = () => {
    timers.splice(0).forEach(clearTimeout);
    store.setReplay([]);
    setReplaying(false);
  };
  const replay = () => {
    stopReplay();
    const list = chips();
    if (!list.length) return;
    setReplaying(true);
    // the same commands, same order; long pauses are shortened to 2 seconds
    let t = 0;
    let prev = list[0].at;
    const playing = new Set<string>();
    const at = (ms: number, fn: () => void) => timers.push(window.setTimeout(fn, ms));
    for (const c of list) {
      t += Math.min(2000, c.at - prev);
      prev = c.at;
      at(t, () => {
        dots.send(ROUTE_SHOW);
        if (c.action === "stop") playing.delete(c.lookId);
        else playing.add(c.lookId);
        store.setReplay([...playing]);
      });
      if (c.action === "flash") {
        at(t + (c.dur ?? 500), () => {
          playing.delete(c.lookId);
          store.setReplay([...playing]);
        });
      }
    }
    at(t + 2500, () => {
      store.setReplay([]);
      setReplaying(false);
    });
  };
  onCleanup(stopReplay);

  // ---- who wins, per group: every layer that speaks, top first
  const speakers = (groupId: string) => {
    const g = groupId as "movers" | "pars" | "bars" | "laser";
    const manual = [...store.runningManual()].reverse().find((b) => lookById(b.lookId).groups.includes(g));
    const show = store.showLooksNow().find((id) => lookById(id).groups.includes(g));
    const auto = store.autopilotNow().find((l) => l.groups.includes(g));
    return [
      manual && { layer: "manual" as const, look: lookById(manual.lookId).name },
      show && { layer: "show" as const, look: lookById(show).name },
      auto && { layer: "autopilot" as const, look: auto.name },
    ].filter((x) => !!x);
  };

  const sendAuto = () => dots.send(ROUTE_AUTO);
  // the autopilot sends a command at each new section; show one now so the picture is alive
  const autoTimer = window.setInterval(sendAuto, 8000);
  onCleanup(() => clearInterval(autoTimer));

  return (
    <div class="how-screen">
      <Region id="flow" title="The whole theory in one picture" class="how-flow" aside={<span class="mute">Every frame is computed fresh from what plays on the layers.</span>}>
        <Flow dots={dots} lit={current()?.lit ?? []} />
      </Region>

      <div class="how-row">
        <Region id="try" title="Try it" class="how-try" aside={<span class="mute">Tap to start, hold to flash</span>}>
          <div class="how-pads">
            <For each={PADS}>
              {(id) => (
                <Pad
                  label={lookById(id).name}
                  colors={lookById(id).colors}
                  state={padState(id)}
                  onDown={() => down(id)}
                  onUp={(ms) => up(id, ms)}
                />
              )}
            </For>
          </div>
          <Fader
            orientation="horizontal"
            label="Master"
            value={store.state.master}
            onChange={(v) => {
              store.setMaster(v);
              dots.send(ROUTE_YOU);
            }}
          />
          <div class="how-rec">
            <Button
              variant="danger"
              pressed={recording()}
              onClick={() => {
                if (!recording()) setChips([]);
                setRecording(!recording());
              }}
            >
              {recording() ? "Stop recording" : "Record"}
            </Button>
            <Button variant="layer" layer="show" pressed={replaying()} disabled={recording() || !chips().length} reason={recording() ? "Stop recording first" : "Record something first"} onClick={() => (replaying() ? stopReplay() : replay())}>
              {replaying() ? "Stop replay" : "Replay on Show"}
            </Button>
          </div>
          <ol class="how-chips" aria-label="Written down">
            <For each={chips()} fallback={<li class="mute">{recording() ? "Press a pad: it gets written down here." : "Press Record, then a few pads."}</li>}>
              {(c) => (
                <li class="how-chip">
                  <i style={{ background: lookById(c.lookId).colors[0] }} />
                  {c.action} {lookById(c.lookId).name}
                </li>
              )}
            </For>
          </ol>
        </Region>

        <Region id="wins" title="Who wins" class="how-wins" aside={<span class="mute">Top layer that speaks</span>}>
          <ul class="how-win-list">
            <For each={store.groups}>
              {(g) => (
                <li>
                  <b>{g.name}</b>
                  <Show when={speakers(g.id)[0]} fallback={<span class="mute">dark: no layer speaks</span>}>
                    {(top) => (
                      <span class={`how-winner layer-${top().layer}`}>
                        {LAYER_NAME[top().layer]}: {top().look}
                      </span>
                    )}
                  </Show>
                  <Show when={speakers(g.id).length > 1}>
                    <span class="how-under">
                      under it: {speakers(g.id).slice(1).map((s) => `${LAYER_NAME[s.layer]} ${s.look}`).join(", ")}
                    </span>
                  </Show>
                </li>
              )}
            </For>
          </ul>
        </Region>

        <Region id="walks" title="Three walks" class="how-walks">
          <Segmented
            label="Walk"
            value={walk()}
            onChange={(w) => {
              setWalk(w);
              goStep(0);
            }}
            options={WALKS.map((w) => ({ value: w.id, label: w.name }))}
          />
          <div class="how-step" aria-live="polite">
            <Show when={current()} fallback={<p class="how-say">Pick a walk, then step through it. The picture lights up the part each step is about.</p>}>
              {(s) => (
                <>
                  <span class="how-step-no">
                    Step {step() + 1} of {steps().length}
                  </span>
                  <p class="how-say">{s().say}</p>
                </>
              )}
            </Show>
          </div>
          <div class="how-step-dots" aria-hidden="true">
            <For each={steps()}>{(_, i) => <i classList={{ "is-done": i() < step(), "is-now": i() === step() }} />}</For>
          </div>
          <div class="how-step-actions">
            <Button disabled={step() <= 0} onClick={() => goStep(step() - 1)}>
              Back
            </Button>
            <Show
              when={step() < steps().length - 1}
              fallback={
                <Button variant="primary" onClick={() => goStep(0)}>
                  Again
                </Button>
              }
            >
              <Button variant="primary" onClick={() => goStep(step() + 1)}>
                {step() < 0 ? "Start" : "Next"}
              </Button>
            </Show>
          </div>
        </Region>
      </div>
    </div>
  );
}
