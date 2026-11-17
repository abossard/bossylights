import { createSignal, For, onCleanup, Show } from "solid-js";
import { lookById, PROPS, store } from "../../fake/store";
import type { GroupId } from "../../fake/data";
import { Button, Fader, Pad, Region, Segmented, type PadState } from "../../kit";
import type { Issue } from "../../shell/registry";
import { createDots, Flow } from "./Flow";
import "./how.css";

export const id = "how";
export const title = "How it works";
export const order = 7;
export const issues: Issue[] = [
  { number: 100, title: "Review the theory and the module boundary map" },
  { number: 72, title: "Who wins: manual, show, autopilot" },
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
      { say: "The show replays the same commands.", lit: ["showw", "p-showw", "show", "p-show", "frame"], route: ["p-showw", "p-show"] },
      { say: "Press a pad: manual beats show, you win.", lit: ["you", "p-you", "manual", "p-manual", "frame"], route: ["p-you", "p-manual"] },
      { say: "Press Release: the show is back.", lit: ["show", "p-show", "frame"], route: ["p-show"] },
    ],
  },
  {
    id: "yield",
    name: "The autopilot yields",
    steps: [
      { say: "The autopilot plays a rising look on every light.", lit: ["autow", "p-autow", "auto", "p-auto", "frame"], route: ["p-autow", "p-auto"] },
      { say: "You press Strobe: you now hold the brightness of the PARs.", lit: ["you", "p-you", "manual"], route: ["p-you"] },
      { say: "Their brightness is yours. Their colour and the movers stay with the autopilot.", lit: ["manual", "auto", "p-manual", "p-auto", "frame"] },
      { say: "Press Release: the PARs go back to the autopilot, no cut.", lit: ["auto", "p-auto", "frame"], route: ["p-auto"] },
    ],
  },
];

interface Chip {
  lookId: string;
  action: "start" | "stop" | "flash";
  at: number;
  dur?: number;
}

const RANK_NAME = { manual: "Manual", show: "Show", autopilot: "Autopilot" } as const;

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

  // ---- replay as the show
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

  // ---- who wins, per group: asked from the one rule table, property by property
  const decided = (groupId: string) => {
    const g = groupId as GroupId;
    const out: { rank: "manual" | "show" | "autopilot"; look: string; props: string[]; note: string }[] = [];
    for (const prop of PROPS) {
      const w = store.winner(g, prop);
      if (!w) continue;
      const same = out.find((x) => x.rank === w.rank && x.look === w.look.name);
      if (same) same.props.push(prop);
      else out.push({ rank: w.rank, look: w.look.name, props: [prop], note: w.padOff ? "pad off, held until Release" : w.left ? "ended, stays" : "" });
    }
    return out;
  };

  const sendAuto = () => dots.send(ROUTE_AUTO);
  // the autopilot sends a command at each new section; show one now so the picture is alive
  const autoTimer = window.setInterval(sendAuto, 8000);
  onCleanup(() => clearInterval(autoTimer));

  return (
    <div class="how-screen">
      <Region id="flow" title="The whole theory in one picture" class="how-flow" aside={<span class="mute">Every frame is computed fresh from what plays: manual over show over autopilot.</span>}>
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
            <Button variant="layer" layer="manual" disabled={!store.holds().length} reason="You hold nothing" onClick={() => store.releaseAll()}>
              Release
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

        <Region id="wins" title="Who wins" class="how-wins" aside={<span class="mute">Manual beats show beats autopilot</span>}>
          <ul class="how-win-list">
            <For each={store.groups}>
              {(g) => (
                <li>
                  <b>{g.name}</b>
                  <For each={decided(g.id)} fallback={<span class="mute">dark: nobody wants it</span>}>
                    {(d) => (
                      <span class="how-win-row">
                        <span class={`how-winner layer-${d.rank}`}>
                          {RANK_NAME[d.rank]}: {d.look}
                        </span>
                        <span class="how-under">
                          {d.props.join(" and ")}
                          {d.note ? `, ${d.note}` : ""}
                        </span>
                      </span>
                    )}
                  </For>
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
