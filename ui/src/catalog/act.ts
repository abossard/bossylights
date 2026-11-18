// What a gesture on a frame does: the look after it and the commands sent. A calculation.
// The catalog has no engine: this stands in for one that always says yes, so a frame can be played.
import type { CommandName } from "../controls/commands.ts";
import { MACHINES, step, type Machine } from "../controls/machines.ts";
import type { Control } from "../controls/spec.ts";
import { inState, type Look } from "./look.ts";
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
  | { is: "drop" }
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
      return undefined;
  }
}

/** The look after a gesture and what was sent. A control that is gone or off does nothing. */
export function act(control: Control, look: Look, gesture: Act): Result {
  const nothing: Result = { look, sent: [] };
  if (look.gone || (look.off && !look.offOption)) return nothing;
  // Learn waits: the press picks the control and does not fire it.
  if (look.learn) return gesture.is === "press" || gesture.is === "down" ? { look, sent: [{ name: "StartLearn", value: control.id }] } : nothing;
  const plain = plainAct(control, look, gesture);
  const use = USES[control.id];
  const kind = gestureOf(gesture);
  const events = use && kind ? use.on[kind] : undefined;
  // The gesture changes the look, the machine keeps its own place.
  return use && events ? machineAct(control, { ...plain.look, at: look.at }, use, events) : plain;
}

/** A control with a machine: the events run through it, the machine names what is sent. */
function machineAct(control: Control, look: Look, use: Use, events: readonly string[]): Result {
  const machine = MACHINES[use.machine] as Machine<string, string>;
  const first = control.sends[0];
  const sent: Sent[] = [];
  let at = look.at || machine.start;
  for (const event of events) {
    const from = at;
    const next = step(machine, from, event);
    at = next.to;
    for (const name of next.sends ?? []) sent.push({ name, value: name === "Blackout" ? `on: ${at !== "off"}` : "" });
    // A lamp sends its own command. A drop sends the command of what was dragged, once.
    if (use.machine === "indicator" && event === "press" && first) sent.push({ name: first, value: `on: ${at === "ready" || at === "on"}` });
    if (use.machine === "drag" && from === "dragging" && event === "up" && first) sent.push({ name: first, value: "where it landed" });
  }
  const shows = use.shows?.[at];
  // Not listed: the look stays. Empty: back to plain, with what the gesture itself changed.
  const next = shows === undefined ? look : shows === "" ? { ...look, variant: "" } : inState(control, shows);
  return { look: { ...next, at }, sent };
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
      return { look: { ...look, value: gesture.value, value2: gesture.value2 }, sent: send(control.kit === "XY" ? `pan ${gesture.value}, tilt ${gesture.value2}` : String(gesture.value)) };
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
    case "release":
    case "key":
    case "lift":
    case "drop":
    case "cancel":
      return { look, sent: [] };
  }
}

/** A toggle goes to the other of its first two states. With two commands the first turns it on, the second off. */
function flip(control: Control, look: Look): Result {
  const other = control.states
    .slice(0, 2)
    .map((state) => inState(control, state))
    .find((state) => state.on !== look.on || state.label !== look.label);
  const next = other ?? { ...look, on: !look.on };
  const [onCommand, offCommand] = control.sends;
  if (!onCommand) return { look: next, sent: [] };
  return { look: next, sent: [offCommand ? { name: next.on ? onCommand : offCommand, value: "" } : { name: onCommand, value: `on: ${next.on}` }] };
}
