// What every component of the kit shares (issue #102).
// A component is a view: props in, a gesture out through a callback. It imports no engine and keeps no state
// of the show. The screen that uses it turns the gesture into a command.

/**
 * Whose colour a fill is. Never decoration (DESIGN.md, the Row Colour Rule).
 *
 * | tone      | means                                   |
 * |-----------|-----------------------------------------|
 * | plain     | nothing special                         |
 * | paper     | now, or the one main action             |
 * | manual    | your hands                              |
 * | show      | the show                                |
 * | autopilot | the autopilot                           |
 * | danger    | blackout, recording, trouble            |
 */
export type Tone = "plain" | "paper" | "manual" | "show" | "autopilot" | "danger";

/** What every control that can be pressed takes. */
export interface Pressable {
  /** Why it cannot be pressed now. Shown under its label: a control never goes off without its reason. */
  off?: string;
  /** A hardware control is mapped to it: a 9 px paper square in its corner. */
  mapped?: boolean;
}

/** "Chase L-R" must not break at its hyphen. */
export const unbroken = (text: string) => text.replace(/-/g, "‑");

/** A value kept inside 0 to 100, in whole steps. */
export const level = (value: number) => Math.max(0, Math.min(100, Math.round(value)));

/** Where a pointer is inside a box, as 0 to 100 from the left and from the bottom. */
export function placeIn(box: { left: number; top: number; width: number; height: number }, x: number, y: number): { across: number; up: number } {
  return {
    across: box.width > 0 ? level(((x - box.left) / box.width) * 100) : 0,
    up: box.height > 0 ? level((1 - (y - box.top) / box.height) * 100) : 0,
  };
}

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
