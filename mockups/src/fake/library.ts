// The looks library: steps, sequences and patterns (docs/design/looks.md).
// Every look the Console plays lives here; the older flat looks become
// steps and patterns, and the new items make the states in the brief real.
import { LOOKS, type GroupId, type Look, type LookKind } from "./data";

export type Speed = 0.5 | 1 | 2;
export type Shape = "plasma" | "stripes" | "wave" | "sparkle";

/** One use of a thing inside a sequence, with its own use settings. In beats. */
export interface Entry {
  id: string;
  ref: string;
  start: number;
  width: number;
  speed: Speed;
  loop: boolean;
  /** 0..100: 0 all fixtures together, 100 spread evenly over the item */
  spread: number;
  /** 0..100: share of the width that is the move, the rest holds */
  transition: number;
}

interface Base {
  id: string;
  name: string;
  /** labels; one thing can be in many groups */
  groups: string[];
  /** how the rest of the app treats it (autopilot style filters) */
  lookKind: LookKind;
}

export interface Step extends Base {
  kind: "step";
  fixtures: GroupId[];
  colour?: string[];
  position?: { x: number; y: number };
  gobo?: string;
  dimmer?: number;
  spin?: number;
}

export interface Sequence extends Base {
  kind: "sequence";
  entries: Entry[];
}

export interface Pattern extends Base {
  kind: "pattern";
  shape: Shape;
  measure: number;
  /** the step that gives the colours, or null: its own colours */
  colours: string | null;
  ownColours: string[];
  on: GroupId;
}

export type Item = Step | Sequence | Pattern;

export const GOBOS = ["leaf", "dots", "ring", "star", "bars", "flower"] as const;

let n = 0;
const e = (ref: string, start: number, width: number, more: Partial<Entry> = {}): Entry => ({
  id: `e${++n}`,
  ref,
  start,
  width,
  speed: 1,
  loop: true,
  spread: 0,
  transition: 0,
  ...more,
});

const step = (id: string, name: string, s: Partial<Step>): Step => ({
  id,
  name,
  kind: "step",
  groups: [],
  lookKind: "wash",
  fixtures: ["pars"],
  ...s,
});

const seq = (id: string, name: string, entries: Entry[], s: Partial<Sequence> = {}): Sequence => ({
  id,
  name,
  kind: "sequence",
  groups: [],
  lookKind: "wash",
  entries,
  ...s,
});

/** The older flat looks, as steps and patterns. */
function fromLook(l: Look): Item {
  if (l.kind === "movement")
    return { id: l.id, name: l.name, kind: "pattern", groups: ["movement"], lookKind: l.kind, shape: "wave", measure: 8, colours: null, ownColours: l.colors, on: "movers" };
  if (l.kind === "pixels")
    return {
      id: l.id,
      name: l.name,
      kind: "pattern",
      groups: ["pixels"],
      lookKind: l.kind,
      shape: /chase/i.test(l.name) ? "stripes" : /sparkle/i.test(l.name) ? "sparkle" : "plasma",
      measure: 4,
      colours: null,
      ownColours: l.colors,
      on: "bars",
    };
  const dark = l.colors[0] === "#000000";
  return step(l.id, l.name, {
    groups: [l.kind === "strobe" ? "strobes" : l.kind === "laser" ? "laser" : "washes"],
    lookKind: l.kind,
    fixtures: l.groups,
    colour: dark ? undefined : l.colors,
    dimmer: dark ? 0 : l.kind === "strobe" ? 100 : 80,
  });
}

/** The new items: the states the Looks brief asks for. */
const NEW: Item[] = [
  step("red", "Red", { colour: ["#ff2a2a"], fixtures: ["pars"], groups: ["colours"] }),
  step("amber", "Amber", { colour: ["#ffa000"], fixtures: ["pars"], groups: ["colours"] }),
  step("green", "Green", { colour: ["#3ddc97"], fixtures: ["pars"], groups: ["colours"] }),
  step("blue", "Blue", { colour: ["#2a4bff"], fixtures: ["pars"], groups: ["colours"] }),
  // colours only: it covers colours and nothing else
  step("red-alert", "Red alert", { colour: ["#ff2a2a"], fixtures: ["pars", "bars", "movers"], groups: ["colours"] }),
  step("jungle-colours", "Jungle colours", { colour: ["#3ddc97", "#b6ff2e", "#12c2a8", "#f2b705"], fixtures: ["bars"], groups: ["colours"] }),
  step("left-low", "Left low", { position: { x: 20, y: 25 }, fixtures: ["movers"], groups: ["positions"], lookKind: "movement" }),
  step("right-high", "Right high", { position: { x: 80, y: 75 }, fixtures: ["movers"], groups: ["positions"], lookKind: "movement" }),
  step("leaf-gobo", "Leaf gobo", { gobo: "leaf", spin: 30, fixtures: ["movers"], groups: ["gobos"], lookKind: "movement" }),
  seq("colour-chase", "Colour chase", [e("red", 0, 1), e("amber", 1, 1), e("green", 2, 1), e("blue", 3, 1)], { groups: ["chases"] }),
  seq("lazy-chase", "Lazy chase", [e("red", 0, 3), e("amber", 3, 1), e("green", 4, 3), e("blue", 7, 1)], { groups: ["chases"] }),
  seq("mover-run", "Mover run", [e("left-low", 0, 4, { transition: 50 }), e("right-high", 4, 4, { transition: 50 }), e("leaf-gobo", 0, 8)], {
    groups: ["movement"],
    lookKind: "movement",
  }),
  seq("flicker", "Flicker", [e("flash-white", 0, 0.25), e("blackout-hold", 0.25, 0.25)], { groups: ["end of build-up", "strobes"], lookKind: "strobe" }),
  seq(
    "build-up-end-a",
    "Build-up end A",
    [e("mover-run", 0, 4, { speed: 2 }), e("colour-chase", 0, 4), e("flicker", 2, 2, { loop: true })],
    { groups: ["end of build-up"], lookKind: "movement" },
  ),
  seq("build-up-end-b", "Build-up end B", [e("mover-run", 0, 8), e("lazy-chase", 0, 8)], { groups: ["end of build-up"], lookKind: "movement" }),
  seq("drop-opener", "Drop opener", [e("mover-run", 0, 4, { speed: 2 }), e("plasma", 0, 4)], { groups: ["drop"], lookKind: "movement" }),
  { id: "plasma", name: "Plasma", kind: "pattern", groups: ["pixels", "drop"], lookKind: "pixels", shape: "plasma", measure: 8, colours: "jungle-colours", ownColours: [], on: "bars" },
];

/** The library as it is when the mockup opens. */
export function initialLibrary(): Record<string, Item> {
  const out: Record<string, Item> = {};
  for (const l of LOOKS) out[l.id] = fromLook(l);
  for (const i of NEW) out[i.id] = i;
  return out;
}
