// What every primitive of the kit shares (issue #102).
// A primitive is a view: props in, a gesture out through a callback. It imports no engine and keeps no state
// of the show. The screen that uses it turns the gesture into a command.

/**
 * Whose colour a fill is. One prop on every primitive, never decoration (DESIGN.md, the Row Colour Rule).
 *
 * | colour    | means                                   |
 * |-----------|-----------------------------------------|
 * | plain     | nothing special                         |
 * | paper     | now, or the one main action             |
 * | manual    | your hands                              |
 * | show      | the show                                |
 * | autopilot | the autopilot                           |
 * | danger    | blackout, recording, trouble            |
 */
export type Colour = "plain" | "paper" | "manual" | "show" | "autopilot" | "danger";

export const COLOURS: readonly Colour[] = ["plain", "paper", "manual", "show", "autopilot", "danger"];

/**
 * What a lamp can say on top of the colours. A lamp only, never a fill (owner's review of the catalog, #102).
 *
 * | signal | means                    |
 * |--------|--------------------------|
 * | mute   | ready, not in effect yet |
 * | ok     | connected, all is well   |
 * | warn   | look here: it is trying  |
 */
export type Signal = Colour | "mute" | "ok" | "warn";

/**
 * | state | means                              | look                                   |
 * |-------|------------------------------------|----------------------------------------|
 * | off   | not in effect                      | outline. A lamp is a dashed ring, its name struck |
 * | ready | will be in effect when LIVE goes on | outline. A lamp is a mute ring, "Ready" |
 * | on    | in effect now                      | filled in its colour. A lamp is solid, "On" |
 */
export type ToggleState = "off" | "ready" | "on";

/** A lamp's shape, the word under its name and what a screen reader hears after the name. Ready and On differ in shape and word, not only in fill (#194). */
export interface LampLook {
  fill: "solid" | "outline" | "dashed";
  word: string;
  says: string;
}

const LAMPS: Readonly<Record<ToggleState, Readonly<LampLook>>> = Object.freeze({
  off: Object.freeze({ fill: "dashed", word: "", says: "Off" }),
  ready: Object.freeze({ fill: "outline", word: "Ready", says: "Ready, starts with LIVE" }),
  on: Object.freeze({ fill: "solid", word: "On", says: "On" }),
});

export const lampOf = (state: ToggleState): Readonly<LampLook> => LAMPS[state];

/**
 * How a thing stands in its place. Item and Block share it.
 *
 * | state  | means                                    | look                 |
 * |--------|------------------------------------------|----------------------|
 * | plain  | there                                    |                      |
 * | picked | this one: open, selected                 | paper                |
 * | lifted | being dragged                            | paper outline        |
 * | dashed | not now: planned, waiting, changing      | dashed outline       |
 * | clash  | it overlaps another one                  | a red bar            |
 */
export type Standing = "plain" | "picked" | "lifted" | "dashed" | "clash";

/** What every primitive that can be pressed takes. */
export interface Pressable {
  /** Why it cannot be pressed now. Shown under its label: a control never goes off without its reason. */
  off?: string;
  /** A hardware control is mapped to it: a 9 px paper square in its corner. */
  mapped?: boolean;
}

/** `{ off }` when there is a reason, nothing when there is none: an optional prop is left out, never undefined. */
export const offWith = (reason: string | undefined): { off?: string } => (reason === undefined ? {} : { off: reason });

/** "Chase L-R" must not break at its hyphen. */
export const unbroken = (text: string) => text.replace(/-/g, "‑");

/** A value kept inside 0 to 100, in whole steps. */
export const level = (value: number) => scaled(value, 100);
/** A value on a range from 0 to `most`: rounded, kept inside. */
export const scaled = (value: number, most: number) => Math.max(0, Math.min(most, Math.round(value)));

/** Where a pointer is inside a box, as 0 to 100 from the left and from the bottom. */
export function placeIn(box: { left: number; top: number; width: number; height: number }, x: number, y: number): { across: number; up: number } {
  return {
    across: box.width > 0 ? level(((x - box.left) / box.width) * 100) : 0,
    up: box.height > 0 ? level((1 - (y - box.top) / box.height) * 100) : 0,
  };
}

/** Where a pointer is on a wheel: `around` 0 to 100 clockwise from the top, `out` 0 in the middle to 100 at the rim and beyond it. */
export function wheelAt(box: { left: number; top: number; width: number; height: number }, x: number, y: number): { around: number; out: number } {
  const rx = box.width / 2;
  const ry = box.height / 2;
  if (rx <= 0 || ry <= 0) return { around: 0, out: 0 };
  const dx = (x - box.left - rx) / rx;
  const dy = (y - box.top - ry) / ry;
  // The middle has no direction: it reads as the top.
  const turn = dx === 0 && dy === 0 ? 0 : (Math.atan2(dx, -dy) / (2 * Math.PI) + 1) % 1;
  return { around: level(turn * 100) % 100, out: level(Math.hypot(dx, dy) * 100) };
}

/** Where a value sits on a wheel: percent of its box, from the left and from the top. */
export function wheelSpot(around: number, out: number): { left: number; top: number } {
  const angle = (around / 100) * 2 * Math.PI;
  return { left: Math.round(50 + (out / 2) * Math.sin(angle)), top: Math.round(50 - (out / 2) * Math.cos(angle)) };
}

/** The colour of a place on the colour wheel: the hue around, white in the middle, full at the rim. */
export const wheelColour = (around: number, out: number): string => `hsl(${Math.round(around * 3.6)} 100% ${Math.round(100 - out / 2)}%)`;

/** What an arrow key does to a value: 1 a press, 10 with Shift or a page key, the ends with Home and End. */
export function keyStep(key: string, shift: boolean): number | "start" | "end" | undefined {
  const by = shift ? 10 : 1;
  switch (key) {
    case "ArrowUp":
    case "ArrowRight":
      return by;
    case "ArrowDown":
    case "ArrowLeft":
      return -by;
    case "PageUp":
      return 10;
    case "PageDown":
      return -10;
    case "Home":
      return "start";
    case "End":
      return "end";
    default:
      return undefined;
  }
}

/** The option an arrow key moves to in a group of choices. It wraps around. */
export function neighbour<T>(options: readonly T[], selected: T, key: string): T | undefined {
  const at = options.indexOf(selected);
  const by = key === "ArrowRight" || key === "ArrowDown" ? 1 : key === "ArrowLeft" || key === "ArrowUp" ? -1 : 0;
  if (by === 0 || options.length === 0) return undefined;
  return options[(Math.max(at, 0) + by + options.length) % options.length];
}
