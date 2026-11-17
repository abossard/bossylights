// The widgets a console page is built from, and what each one draws.
import { createSignal, For, Match, Switch } from "solid-js";
import { lookById, store, type FaderId } from "../../fake/store";
import { Button, Fader, Pad, Region, Segmented, XYPad, type PadState } from "../../kit";

export type WidgetKind = "pads" | "faders" | "xy" | "button" | "slider" | "cuelist" | "speed" | "label" | "frame";

export interface Widget {
  id: string;
  kind: WidgetKind;
  title: string;
  x: number;
  y: number;
  w: number;
  h: number;
  /** pads: which bank. button: which look. slider: which fader. */
  ref?: string;
}

export const COLS = 12;
export const ROWS = 12;

export const KINDS: Record<WidgetKind, { name: string; about: string; w: number; h: number; minW: number; minH: number }> = {
  pads: { name: "Pad grid", about: "A grid of look pads", w: 12, h: 6, minW: 6, minH: 3 },
  faders: { name: "Fader bank", about: "Master and group levels", w: 7, h: 6, minW: 4, minH: 4 },
  xy: { name: "XY pad", about: "Drag to aim the movers", w: 4, h: 4, minW: 3, minH: 3 },
  button: { name: "Button", about: "One pad for one look", w: 2, h: 2, minW: 2, minH: 2 },
  slider: { name: "Slider", about: "One fader", w: 2, h: 5, minW: 2, minH: 4 },
  cuelist: { name: "Cue list", about: "Looks in order, with Go", w: 4, h: 5, minW: 3, minH: 4 },
  speed: { name: "Speed dial", about: "Half, normal or double speed", w: 4, h: 2, minW: 3, minH: 2 },
  label: { name: "Label", about: "A word on the surface", w: 3, h: 1, minW: 2, minH: 1 },
  frame: { name: "Frame", about: "An outline to group widgets", w: 6, h: 6, minW: 2, minH: 2 },
};

/** Kinds the drawer offers. */
export const DRAWER: WidgetKind[] = ["button", "slider", "xy", "cuelist", "speed", "label", "frame"];

const FADER_LABEL: Record<FaderId | "master", string> = {
  master: "Master",
  wash: "Wash",
  movers: "Movers",
  pixels: "Pixels",
  strobeRate: "Strobe rate",
  laser: "Laser",
};

export const BANK_FADERS: Record<string, (FaderId | "master")[]> = {
  main: ["master", "wash", "movers", "pixels", "strobeRate", "laser"],
  movers: ["master", "movers"],
  pixels: ["master", "pixels", "wash", "strobeRate"],
};

const HOLD_MS = 350;

/** Short press starts or stops a look. A long hold flashes it until let go. */
function usePadPress() {
  const wasRunning = new Set<string>();
  const down = (id: string) => {
    const s = store.lookState(id);
    if (s === "held" || s === "flash") wasRunning.add(id);
    else store.hold(id);
  };
  const up = (id: string, ms: number) => {
    if (wasRunning.delete(id)) {
      if (ms < HOLD_MS) store.stop(id);
    } else if (ms < HOLD_MS) store.start(id);
    else store.release(id);
  };
  return { down, up };
}

const padState = (id: string): PadState => store.lookState(id) ?? "idle";

function LookPad(props: { id: string; disabled?: boolean }) {
  const press = usePadPress();
  const look = () => lookById(props.id);
  return (
    <Pad
      label={look().name}
      colors={look().colors}
      state={padState(props.id)}
      disabled={props.disabled}
      onDown={() => press.down(props.id)}
      onUp={(ms) => press.up(props.id, ms)}
    />
  );
}

function faderValue(id: FaderId | "master") {
  return id === "master" ? store.state.master : store.state.faders[id];
}
function setFaderValue(id: FaderId | "master", v: number) {
  if (id === "master") store.setMaster(v);
  else store.setFader(id, v);
}

function CueList(props: { editing: boolean }) {
  const cues = ["warm-wash", "sunset", "pulse", "slow-fade"];
  const [at, setAt] = createSignal(-1);
  const goTo = (i: number) => {
    const prev = at();
    if (prev >= 0) store.stop(cues[prev]);
    if (i >= 0 && i < cues.length) store.start(cues[i]);
    setAt(i >= cues.length ? -1 : i);
  };
  return (
    <div class="w-cues">
      <ol>
        <For each={cues}>
          {(id, i) => (
            <li classList={{ "is-current": at() === i() }}>
              <span class="w-cue-n">{i() + 1}</span>
              {lookById(id).name}
              {at() === i() ? <span class="now-flag">Now</span> : null}
            </li>
          )}
        </For>
      </ol>
      <div class="w-cue-actions">
        <Button disabled={props.editing || at() < 0} onClick={() => goTo(at() - 1)}>
          Back
        </Button>
        <Button variant="layer" layer="manual" size="lg" disabled={props.editing} onClick={() => goTo(at() + 1)}>
          Go
        </Button>
      </div>
    </div>
  );
}

function SpeedDial() {
  const [speed, setSpeed] = createSignal<"half" | "one" | "double">("one");
  const bpm = () => store.state.tempo * { half: 0.5, one: 1, double: 2 }[speed()];
  return (
    <div class="w-speed">
      <Segmented
        label="Speed"
        size="lg"
        value={speed()}
        onChange={setSpeed}
        options={[
          { value: "half", label: "Half" },
          { value: "one", label: "Normal" },
          { value: "double", label: "Double" },
        ]}
      />
      <span class="mute">Chases run at {bpm()} BPM</span>
    </div>
  );
}

/** One widget's content. Each widget is a Region, so it can be reviewed. */
export function WidgetBody(props: { widget: Widget; editing: boolean; pageId: string }) {
  const w = () => props.widget;
  return (
    <Switch>
      <Match when={w().kind === "pads"}>
        {(() => {
          const bank = () => store.state.pads[w().ref ?? "main"] ?? store.state.pads.main;
          return (
            <Region id={`${props.pageId}-${w().id}`} title={w().title} bare class="w-pads">
              <div class="pads" style={{ "--pad-rows": Math.ceil(bank().length / 8) }}>
                <For each={bank()}>{(id) => <LookPad id={id} disabled={props.editing} />}</For>
              </div>
            </Region>
          );
        })()}
      </Match>
      <Match when={w().kind === "faders"}>
        <Region id={`${props.pageId}-${w().id}`} title={w().title} aside={<span class="mute">{props.pageId === "main" ? "Main page" : ""}</span>} class="w-faders">
          <div class="faders">
            <For each={BANK_FADERS[w().ref ?? "main"] ?? BANK_FADERS.main}>
              {(id) => <Fader label={FADER_LABEL[id]} value={faderValue(id)} onChange={(v) => setFaderValue(id, v)} />}
            </For>
          </div>
        </Region>
      </Match>
      <Match when={w().kind === "xy"}>
        <Region
          id={`${props.pageId}-${w().id}`}
          title={w().title}
          aside={<span class="mute">Pan {store.state.xy.pan} Tilt {store.state.xy.tilt}</span>}
          class="w-xy"
        >
          <XYPad label="Movers position" x={store.state.xy.pan} y={store.state.xy.tilt} onChange={(x, y) => store.setXY(x, y)} />
        </Region>
      </Match>
      <Match when={w().kind === "button"}>
        <Region id={`${props.pageId}-${w().id}`} title={w().title} bare class="w-button">
          <LookPad id={w().ref ?? "blinder"} disabled={props.editing} />
        </Region>
      </Match>
      <Match when={w().kind === "slider"}>
        <Region id={`${props.pageId}-${w().id}`} title={w().title} bare class="w-slider">
          {(() => {
            const id = (w().ref ?? "strobeRate") as FaderId | "master";
            return <Fader label={FADER_LABEL[id]} value={faderValue(id)} onChange={(v) => setFaderValue(id, v)} />;
          })()}
        </Region>
      </Match>
      <Match when={w().kind === "cuelist"}>
        <Region id={`${props.pageId}-${w().id}`} title={w().title} class="w-cuelist">
          <CueList editing={props.editing} />
        </Region>
      </Match>
      <Match when={w().kind === "speed"}>
        <Region id={`${props.pageId}-${w().id}`} title={w().title} class="w-speedr">
          <SpeedDial />
        </Region>
      </Match>
      <Match when={w().kind === "label"}>
        <Region id={`${props.pageId}-${w().id}`} title={w().title} bare class="w-label">
          <p class="display">{w().title}</p>
        </Region>
      </Match>
      <Match when={w().kind === "frame"}>
        <Region id={`${props.pageId}-${w().id}`} title={w().title} bare class="w-frame">
          <span class="w-frame-title display">{w().title}</span>
        </Region>
      </Match>
    </Switch>
  );
}
