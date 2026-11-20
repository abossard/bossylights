// How a control looks in one frame of the catalog: data. A calculation from the row of the behaviour table,
// a state by its name or a situation by its cell. The frame draws a `Look` with the kit and nothing else.
import { MACHINES } from "../controls/machines.ts";
import type { SituationId } from "../controls/situations.ts";
import type { Control } from "../controls/spec.ts";
import type { Colour } from "../kit/common.ts";
import { SAMPLES, SITUATION_STATE, STATE_LOOKS } from "./samples.ts";
import { USES } from "./uses.ts";

export interface Look {
  /** The words on it. */
  label: string;
  /** A second, quieter line: a value as it reads, a kind, a state in words. */
  sub: string;
  /** Whose it is. */
  colour: Colour;
  /** Filled: on, selected, open. */
  on: boolean;
  /** Why it cannot be pressed. Empty: it can. */
  off: string;
  /** With `off`: only this option is off, not the whole control. */
  offOption: string;
  /** Why it is not on screen. Empty: it is. */
  gone: string;
  /** 0 to 100: a level, a pan. For a fold: how much is inside. */
  value: number;
  /** 0 to 100: a tilt. */
  value2: number;
  options: readonly string[];
  /** The option that is picked, or the text in a field. */
  picked: string;
  /** A named look of its component: the pad's state, the bar's, the cell's, the lamp. Empty: the plain one. */
  variant: string;
  /** The state of its machine. Empty: it has none. */
  at: string;
  /** Learn waits: a press picks it and does not fire it. */
  learn: boolean;
}

const PLAIN: Look = { label: "", sub: "", colour: "plain", on: false, off: "", offOption: "", gone: "", value: 62, value2: 40, options: [], picked: "", variant: "", at: "", learn: false };

/** The usual look of a control: nothing special is true. */
export function usual(control: Control): Look {
  const sample = SAMPLES[control.id] ?? {};
  // A group of segments whose states are its options needs no sample.
  const options = sample.options ?? (control.kit === "Picker.segments" ? control.states : []);
  const use = USES[control.id];
  const at = use ? (use.at ?? MACHINES[use.machine].start) : "";
  return { ...PLAIN, label: control.name, options, picked: options[0] ?? "", ...sample, at };
}

const capital = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);
const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/** States that are on, by their name. */
const ON = /^(on|selected|open|shown|picked|playing|in use|set|may use|muted|flashing|form shown|running|latched|held|something runs|menu open|unfolded)$/i;
/** Words on a toggle that say it is on. */
const ON_WORDS = /^(Stop|Done|Testing|Listening|Fold back|Lights up|Cancel|Sent|Continue)/;
/** States that are the usual look. */
const USUAL = /^(plain|idle|there|both|free|works|off|left alone|plays|folded|nothing runs|at a value|has a step|something held|one selected|not picked|ready|not started|pad on)$/i;

const PAD: Record<string, string> = { idle: "idle", "running by hand": "manual", running: "manual", flash: "flash", "pad off, still held": "kept", "run by the show": "show", "run by the autopilot": "autopilot" };
const LAMP: Record<string, string> = { running: "mute", "live on": "paper", trouble: "danger", "not connected": "empty" };

/** The reason a row gives for being off, when a state is named after it: "nothing held", "locked". */
function reasonNamed(control: Control, state: string): string {
  const cell = Object.values(control.cells).find((c) => c.is === "off" && c.says.toLowerCase().startsWith(state.toLowerCase()));
  return cell?.says ?? "";
}

/** The look of a control in one of its states. `state`: an entry of the row's `states`, or what a machine state shows as. */
export function inState(control: Control, state: string): Look {
  const base = usual(control);
  const look = { ...base, at: atFor(control, state) ?? base.at };
  if (state === "") return look;
  const written = STATE_LOOKS[control.id]?.[state];
  if (written) return { ...look, ...written };

  const gone = /^gone:? (.+)$/.exec(state);
  if (gone?.[1]) return { ...look, gone: capital(gone[1]) };

  // off, with the reason
  const side = /^(.+) off at (.+)$/.exec(state) ?? /^(first|last)(: no '.+')$/.exec(state);
  if (side?.[1] && side[2]) {
    const end = side[1] === "first" ? 0 : side[1] === "last" ? 1 : look.options.findIndex((o) => same(o, side[1] ?? ""));
    const reason = side[1] === "first" ? "It is the first" : side[1] === "last" ? "It is the last" : `At ${side[2]}`;
    return { ...look, off: reason, offOption: look.options[Math.max(end, 0)] ?? "" };
  }
  const off = /^off(?::? (.+))?$/.exec(state);
  const toggles = control.states.some((s) => same(s, "on"));
  if (off && !(off[1] === undefined && toggles)) {
    const reason = off[1] ? capital(off[1]) : (Object.values(control.cells).find((c) => c.is === "off" && c.says !== "Engine not connected")?.says ?? "Off");
    return control.kit === "Picker.cards" ? { ...look, off: reason, offOption: look.options.at(-1) ?? "" } : { ...look, off: reason };
  }
  const named = reasonNamed(control, state);
  if (named) return { ...look, off: named };

  // one of its options
  const option = look.options.find((o) => same(o, state));
  if (option !== undefined) return { ...look, picked: option };
  if (same(state, "not picked")) return { ...look, picked: look.options[1] ?? "" };
  if (same(state, "marked: trouble")) return { ...look, variant: "marked" };

  if (control.kit === "Fader" && /^at \d+$/.test(state)) return { ...look, value: Number(state.slice(3)) };
  if (control.kit === "Pad") return { ...look, variant: PAD[state] ?? "idle", on: (PAD[state] ?? "idle") !== "idle" };
  if (control.id === "menubar.item" && LAMP[state.toLowerCase()]) return { ...look, variant: LAMP[state.toLowerCase()] ?? "", sub: capital(state) };
  if (control.kit === "Button.toggle" && same(state, "empty")) return { ...look, on: true, value: 0 };
  if (same(state, "a look is dragged over it")) return { ...look, variant: "over" };
  if (same(state, "dragged")) return { ...look, variant: "lifted" };
  if (/clash/i.test(state)) return { ...look, variant: "clash", sub: look.sub || "Clash" };
  if (same(state, "held by hand")) return { ...look, variant: "held" };
  if (same(state, "changing")) return { ...look, variant: "changing" };
  if (same(state, "no looks")) return { ...look, variant: "dashed", sub: "No looks" };
  if (same(state, "waits for LIVE")) return { ...look, variant: "ready", sub: "Waits for LIVE" };

  if (ON.test(state)) return { ...look, on: true, variant: state.toLowerCase() };
  // The fill of an action is its colour, not a state: it stays.
  if (USUAL.test(state)) return { ...look, on: control.kit === "Button.action" && look.on, variant: same(state, "off") || same(state, "ready") ? state.toLowerCase() : look.variant };
  // The words on it: "Record", "Stop recording".
  if (/^[A-Z÷×]/.test(state)) return { ...look, label: state, on: ON_WORDS.test(state) };
  // Anything else is said in words under the label.
  return { ...look, sub: capital(state), variant: state.toLowerCase() };
}

/** The state of the machine a state name stands for, when the control has a machine. */
function atFor(control: Control, state: string): string | undefined {
  const shows = USES[control.id]?.shows;
  if (!shows) return undefined;
  return Object.entries(shows).find(([, label]) => label === state)?.[0];
}

/** The look of a control in one situation: the cell of the row decides if it is there and if it can be pressed. */
export function inSituation(control: Control, situation: SituationId): Look {
  const cell = control.cells[situation];
  const look = inState(control, SITUATION_STATE[control.id]?.[situation] ?? "");
  if (cell.is === "gone") return { ...look, gone: cell.says };
  if (cell.is === "off") return { ...look, off: cell.says, offOption: "" };
  const picked = situation === "learn" && control.learn.yes && cell.says.startsWith("It is picked");
  return { ...look, off: "", offOption: "", gone: "", learn: picked };
}
