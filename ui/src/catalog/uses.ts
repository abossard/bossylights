// How each control with a state machine uses it: which gesture is which event, where the control
// lives in the machine, and what each machine state shows as. Data.
import type { MachineName } from "../controls/machines.ts";

/** The gestures a frame can report. `act.ts` lists what each one carries. */
export type Gesture = "press" | "down" | "upShort" | "upLong" | "release" | "keyDown" | "keyUp" | "lift" | "drop" | "cancel" | "pick";

export interface Use {
  machine: MachineName;
  /** The events a gesture causes, in order. A gesture that is not listed causes none. */
  on: Partial<Record<Gesture, readonly string[]>>;
  /** Where the control lives in its machine, when that is not the start. */
  at?: string;
  /** The name of the row's state each machine state shows as. Empty: the usual look. Not listed: the look stays. */
  shows?: Record<string, string>;
  /** Where it shows otherwise while no song plays: `shows` for those states, in a frame with no song. */
  noSong?: Record<string, string>;
}

const lamp = (ready: string): Use => ({ machine: "indicator", on: { press: ["press"] }, shows: { ready, on: "on", off: "off", offLive: "off" } });
const INDICATOR = lamp("ready");

/** Down, then up short or long: the grammar of the pad. A `Button` that listens to these is drawn as one that is held. */
const PAD_PRESS = { down: ["down"], upShort: ["upShort"], upLong: ["upLong"] } as const;
const PAD_ON = { ...PAD_PRESS, release: ["release"] } as const;

/** A control whose short press and hold mean two things: it reports down and up, never a press. */
export const isHeld = (id: string): boolean => USES[id]?.on.upLong !== undefined;

/** Every drag: lifted after 8 px, dropped, or put back. A press is down and up with no move. */
const DRAG: Use = {
  machine: "drag",
  on: { press: ["down", "up"], lift: ["down", "move"], drop: ["up"], cancel: ["cancel"] },
  shows: { idle: "", armed: "", dragging: "dragged" },
};

const adding = (at: string, on: Use["on"], shows?: Record<string, string>): Use => ({ machine: "addLights", on, at, ...(shows ? { shows } : {}) });

export const USES: Record<string, Use> = {
  "top.indicator.dj": INDICATOR,
  // On with no song it is armed: it writes only while a song plays.
  "top.indicator.recording": { ...INDICATOR, noSong: { on: "armed" } },
  "top.indicator.autopilot": INDICATOR,
  "top.indicator.lock": INDICATOR,
  "console.autopilot.on": lamp("waits for LIVE"),
  "top.blackout": {
    machine: "blackout",
    on: { ...PAD_PRESS, keyDown: ["spaceDown"], keyUp: ["spaceUp"] },
    shows: { off: "off", flash: "held", held: "held", latched: "latched", latchedDown: "latched" },
  },
  "top.tap": { machine: "tempo", on: { press: ["tap"] } },
  "top.tempo.source": { machine: "tempo", on: { press: ["back"] }, at: "tap", shows: { tap: "from Tap", feed: "from the feed", oneTap: "from the feed" } },
  "top.learn": { machine: "learn", on: { upShort: ["press"], upLong: ["hold"], cancel: ["cancel"] }, shows: { off: "off", pick: "pick", wait: "wait" } },
  "console.edit-surface": { machine: "surface", on: { press: ["edit"] }, shows: { playing: "there", locked: "locked", editing: "gone: 'Done editing' is in its place" } },
  "console.done-editing": { machine: "surface", on: { press: ["done"] }, at: "editing", shows: { editing: "", playing: "gone: Not editing", locked: "gone: Not editing" } },
  "console.pad": {
    machine: "pad",
    on: PAD_ON,
    shows: { off: "idle", flash: "flash", on: "running by hand", onDown: "running by hand", kept: "pad off, still held" },
  },
  "looks.try": { machine: "playPad", on: PAD_ON, shows: { off: "idle", flash: "flash", on: "running", onDown: "running" } },
  "how.pad": { machine: "playPad", on: PAD_ON, shows: { off: "idle", flash: "flash", on: "running", onDown: "running" } },
  "console.widget.frame": DRAG,
  "console.widget.resize": DRAG,
  "songs.section.edge": DRAG,
  "looks.library.look": DRAG,
  "looks.seq.bar": DRAG,
  "looks.seq.bar.edge": DRAG,
  "patch.grid.light": DRAG,
  "patch.fixtures.order": DRAG,
  "patch.add.leave": adding("mode", {}),
  "patch.add.search": adding("find", {}),
  "patch.add.model": adding("find", { pick: ["pick"] }),
  "patch.add.mode": adding("mode", { pick: ["pick"] }),
  "patch.add.address": adding("address", {}),
  "patch.add.next": adding("mode", { press: ["next"] }, { mode: "Next: address", address: "Next: flash it", flash: "gone: The light flashes: Yes or Nothing flashed", find: "gone: Pick a model first" }),
  "patch.add.confirm": adding("flash", { press: ["yes"] }),
  "patch.add.nothing-flashed": adding("flash", { press: ["no"] }),
  "patch.add.back": adding("mode", { press: ["back"] }),
  "patch.add.start-over": adding("mode", { press: ["startOver"] }),
};
