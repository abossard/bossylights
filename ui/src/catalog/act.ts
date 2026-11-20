// What a gesture on a frame does: the look after it and the commands sent. A calculation.
// The catalog has no engine: this stands in for one that always says yes, so a frame can be played.
import type { CommandName } from "../controls/commands.ts";
import { MACHINES, step, type Machine } from "../controls/machines.ts";
import type { Control } from "../controls/spec.ts";
import { inState, type Look } from "./look.ts";
import { place, placed, type Move } from "./place.ts";
import { USES, type Gesture, type Use } from "./uses.ts";

/** A gesture, with what it carries. */
export type Act =
  | { is: "press" }
  | { is: "down" }
  /** `long`: it was down for more than a short press. */
  | { is: "up"; long: boolean }
  | { is: "release" }
  /** The key that holds: Space on Blackout. */
  | { is: "key"; down: boolean }
  | { is: "set"; value: number; value2: number }
  | { is: "pick"; option: string }
  | { is: "text"; text: string }
  | { is: "step"; by: -1 | 1 }
  | { is: "lift" }
  /** `by`: how far it went, for a control that is placed. */
  | { is: "drop"; by?: Move }
  /** An arrow key on a control that is placed: one step, with Shift one step wider or narrower. */
  | { is: "nudge"; by: Move }
  | { is: "cancel" }
  | { is: "remove" };

/** A command as a frame shows it: its name and the value the gesture carried. */
export interface Sent {
  name: CommandName;
  value: string;
}

export interface Result {
  look: Look;
  sent: readonly Sent[];
}

function gestureOf(act: Act): Gesture | undefined {
  switch (act.is) {
    case "up":
      return act.long ? "upLong" : "upShort";
    case "key":
      return act.down ? "keyDown" : "keyUp";
    case "press":
    case "down":
    case "release":
    case "pick":
    case "lift":
    case "drop":
    case "cancel":
      return act.is;
    case "set":
    case "text":
    case "step":
    case "remove":
    case "nudge":
      return undefined;
  }
}

/** The look after a gesture and what was sent. A control that is gone or off does nothing. */
export function act(control: Control, look: Look, gesture: Act): Result {
  const nothing: Result = { look, sent: [] };
  if (look.gone || (look.off && !look.offOption)) return nothing;
  // Learn waits: the press picks the control and does not fire it.
  if (look.learn) return gesture.is === "press" || gesture.is === "down" ? { look, sent: [{ name: "StartLearn", value: control.id }] } : nothing;
  const result = plainAct(control, look, gesture);
  // Whether a song plays is a fact of the frame: no gesture changes it.
  const plain = { ...result, look: { ...result.look, song: look.song } };
  const use = USES[control.id];
  const kind = gestureOf(gesture);
  const events = use && kind ? use.on[kind] : undefined;
  // A control with a machine that reports down and up: what its machine does not list does nothing.
  if (use && !events && (gesture.is === "down" || gesture.is === "up")) return nothing;
  // The gesture changes the look, the machine keeps its own place.
  // A drop where it started: a placed thing back at its place, any other thing that went nowhere.
  const stayed = gesture.is === "drop" && (placed(control.id) ? plain.look.sub === look.sub : gesture.by !== undefined && gesture.by.across === 0 && gesture.by.down === 0);
  return use && events ? machineAct(control, { ...plain.look, at: look.at }, use, events, stayed) : plain;
}

/** A control with a machine: the events run through it, the machine names what is sent. `stayed`: it was dropped where it started. */
function machineAct(control: Control, look: Look, use: Use, events: readonly string[], stayed: boolean): Result {
  const machine = MACHINES[use.machine] as Machine<string, string>;
  const first = control.sends[0];
  const sent: Sent[] = [];
  let at = look.at || machine.start;
  for (const event of events) {
    const from = at;
    const next = step(machine, from, event);
    at = next.to;
    for (const name of next.sends ?? []) sent.push({ name, value: name === "Blackout" ? `on: ${at !== "off"}` : "" });
    // A lamp sends its own command. A drop sends the command of what was dragged, once. Dropped where it started, it sends nothing.
    if (use.machine === "indicator" && event === "press" && first) sent.push({ name: first, value: `on: ${at === "ready" || at === "on"}` });
    if (use.machine === "drag" && from === "dragging" && event === "up" && first && !stayed) sent.push({ name: first, value: placed(control.id) ? look.sub : "where it landed" });
  }
  const shows = (look.song ? undefined : use.noSong?.[at]) ?? use.shows?.[at];
  // Not listed: the look stays. Empty: back to plain, with what the gesture itself changed.
  const next = shows === undefined ? look : shows === "" ? { ...look, variant: "" } : inState(control, shows);
  return { look: { ...next, at, song: look.song }, sent };
}

/** A press on these picks the thing: it stays marked. */
const PICKS = new Set<Control["kit"]>(["Item.row", "Item.tile", "Item.cell", "Block.bar"]);

/** A control with no machine: its primitive says what a gesture means, the row names the command. */
function plainAct(control: Control, look: Look, gesture: Act): Result {
  const first = control.sends[0];
  const send = (value = ""): Sent[] => (first ? [{ name: first, value }] : []);
  switch (gesture.is) {
    case "press": {
      if (control.kit === "Button.toggle") return flip(control, look);
      if (control.id === "menubar.item") return { look: { ...look, on: !look.on }, sent: [] };
      return { look: PICKS.has(control.kit) && !first ? { ...look, on: !look.on } : look, sent: send() };
    }
    // A pad with no machine of its own plays in an explainer: down is a flash, a short press latches.
    case "down":
      return { look: { ...look, variant: "flash", on: true }, sent: send() };
    case "up":
      return { look: { ...look, variant: gesture.long ? "idle" : "manual", on: !gesture.long }, sent: [] };
    case "set":
      return { look: { ...look, value: gesture.value, value2: gesture.value2 }, sent: send(control.kit === "XY.position" ? `pan ${gesture.value}, tilt ${gesture.value2}` : String(gesture.value)) };
    case "pick":
      return gesture.option === look.offOption ? { look, sent: [] } : { look: { ...look, picked: gesture.option }, sent: send(gesture.option) };
    case "text":
      return { look: { ...look, picked: gesture.text }, sent: send(gesture.text) };
    case "step": {
      const side = look.options[gesture.by < 0 ? 0 : 1] ?? "";
      return side === look.offOption ? { look, sent: [] } : { look, sent: send(side) };
    }
    case "remove": {
      const name = control.sends.find((n) => n.startsWith("Remove")) ?? first;
      return { look, sent: name ? [{ name, value: "" }] : [] };
    }
    case "nudge": {
      const to = place(control.id, look.sub, gesture.by);
      return to === undefined || to === look.sub ? { look, sent: [] } : { look: { ...look, sub: to }, sent: send(to) };
    }
    // Where a placed control lands. The drag machine sends it.
    case "drop": {
      const to = gesture.by && look.at === "dragging" ? place(control.id, look.sub, gesture.by) : undefined;
      return { look: to === undefined ? look : { ...look, sub: to }, sent: [] };
    }
    case "release":
    case "key":
    case "lift":
    case "cancel":
      return { look, sent: [] };
  }
}

/** A toggle that ends by itself: a press while it runs does nothing, after `ms` it is back. Data. */
export const ENDS_BY_ITSELF: Record<string, { while: string; back: string; ms: number }> = {
  "inout.out.test": { while: "Testing", back: "Test", ms: 3000 },
};

/** A toggle goes to the other of its first two states. With two commands the first turns it on, the second off. */
function flip(control: Control, look: Look): Result {
  if (ENDS_BY_ITSELF[control.id]?.while === look.label) return { look, sent: [] };
  const other = control.states
    .slice(0, 2)
    .map((state) => inState(control, state))
    .find((state) => state.on !== look.on || state.label !== look.label);
  const next = other ?? { ...look, on: !look.on };
  const [onCommand, offCommand] = control.sends;
  if (!onCommand) return { look: next, sent: [] };
  return { look: next, sent: [offCommand ? { name: next.on ? onCommand : offCommand, value: "" } : { name: onCommand, value: `on: ${next.on}` }] };
}
