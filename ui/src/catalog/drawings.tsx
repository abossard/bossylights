// Stand-in pictures for the catalog: gobos, the slots of a colour wheel, matrix effects.
// The real ones come from the fixture library and the looks of the show file. Data and calculations.
import type { JSX } from "solid-js";
import { Disc, Preview, type PickOption } from "../kit/index.ts";

/** A gobo as a drawing, white on black, by its name. */
export const GOBOS: Record<string, () => JSX.Element> = {
  Open: () => (
    <svg viewBox="0 0 40 40">
      <circle cx="20" cy="20" r="16" />
    </svg>
  ),
  Dots: () => (
    <svg viewBox="0 0 40 40">
      <circle cx="20" cy="9" r="4" />
      <circle cx="9" cy="20" r="4" />
      <circle cx="31" cy="20" r="4" />
      <circle cx="20" cy="31" r="4" />
      <circle cx="20" cy="20" r="4" />
    </svg>
  ),
  Star: () => (
    <svg viewBox="0 0 40 40">
      <polygon points="20,3 24.5,14.5 37,15.5 27.5,23.5 30.5,36 20,29 9.5,36 12.5,23.5 3,15.5 15.5,14.5" />
    </svg>
  ),
  Bars: () => (
    <svg viewBox="0 0 40 40">
      <rect x="7" y="6" width="5" height="28" />
      <rect x="17.5" y="6" width="5" height="28" />
      <rect x="28" y="6" width="5" height="28" />
    </svg>
  ),
};

/** Options with the gobo of their name as the picture. A name with no drawing shows the open gobo. */
export const gobos = (names: readonly string[]): PickOption[] => names.map((name) => ({ name, picture: GOBOS[name] ?? GOBOS.Open ?? (() => undefined) }));

/** The slots of a colour wheel: name and its colours. Two colours: a split slot. */
export const WHEEL: readonly (readonly [string, readonly string[]])[] = [
  ["Open", ["#ffffff"]],
  ["Red", ["#ff2a2a"]],
  ["Amber", ["#ffa000"]],
  ["Green", ["#3ddc97"]],
  ["Blue", ["#2a4bff"]],
  ["Red and blue", ["#ff2a2a", "#2a4bff"]],
];
/** Options with the slot of their name as the picture. A name that is not on the wheel has none. */
const slots = (names: readonly string[]): PickOption[] =>
  names.map((name) => {
    const colours = WHEEL.find(([slot]) => slot === name)?.[1];
    return { name, ...(colours ? { picture: () => <Disc colours={colours} /> } : {}) };
  });
export const wheelSlots = (): PickOption[] => slots(WHEEL.map(([name]) => name));

const wave = (x: number) => (Math.sin(x) + 1) / 2;
const light = (hue: number, level: number) => `hsl(${Math.round(hue)} 90% ${Math.round(level * 55)}%)`;

/** Matrix effects: the colour of a pixel at a time. `columns`: how wide the grid is. */
export const EFFECTS: Record<string, (columns: number) => (column: number, row: number, seconds: number) => string> = {
  Plasma: () => (c, r, t) => light(200 + 120 * wave(c / 2.3 + t * 1.7) + 40 * wave(r / 1.7 - t), 0.35 + 0.65 * wave(c / 3 + r / 2 + t * 2)),
  Chase: (columns) => (c, _r, t) => light(40, Math.max(0, 1 - Math.abs(((t * 6) % (columns + 4)) - 2 - c) / 2.5)),
  Rain: () => (c, r, t) => light(190, wave(r * 1.3 - t * 7 + c * 2.1) ** 6),
  Stripes: () => (c, _r, t) => light(320, wave(c * 1.2 - t * 4) > 0.5 ? 1 : 0.1),
  Wave: () => (c, r, t) => light(190 + 4 * r, wave(c / 1.4 - t * 3)),
  Sparkle: () => (c, r, t) => `hsl(0 0% ${Math.round(100 * wave(c * 12.9898 + r * 78.233 + Math.floor(t * 8) * 37.719) ** 12)}%)`,
};

/** Options with an effect running in each: the chooser with preview. `still`: one frame each. */
export const effects = (still = false): PickOption[] =>
  Object.entries(EFFECTS).map(([name, effect]) => ({ name, picture: () => <Preview label={`${name}, running`} columns={8} rows={8} paint={effect(8)} size="small" still={still} /> }));

/** Options with a still preview of their pattern. A name with no pattern shows plasma. */
const patterns = (names: readonly string[]): PickOption[] =>
  names.map((name) => {
    const effect = EFFECTS[name] ?? EFFECTS.Plasma;
    return { name, picture: () => (effect ? <Preview label={`${name}, a still`} columns={8} rows={4} paint={effect(8)} size="small" still /> : undefined) };
  });

/** How the pixels of a panel are wired, as the path through a grid of 4 by 3. */
const WIRING: Record<string, string> = {
  Snake: "M4 4 H36 V14 H4 V24 H36",
  Zigzag: "M4 4 H36 M4 14 H36 M4 24 H36 M36 4 L4 14 M36 14 L4 24",
};
const wirings = (names: readonly string[]): PickOption[] =>
  names.map((name) => ({
    name,
    picture: () => (
      <svg viewBox="0 0 40 28" class="c-drawing">
        <path d={WIRING[name] ?? ""} fill="none" stroke="currentColor" stroke-width="2" />
      </svg>
    ),
  }));

/** A model of light by its outline: a PAR can, a moving head, a strobe. Stand-ins for the library's pictures. */
const MODEL: Record<string, () => JSX.Element> = {
  "LED PAR 64": () => (
    <svg viewBox="0 0 40 28" class="c-drawing">
      <rect x="8" y="6" width="22" height="16" fill="currentColor" />
      <circle cx="31" cy="14" r="6" fill="currentColor" />
    </svg>
  ),
  "Moving head 250": () => (
    <svg viewBox="0 0 40 28" class="c-drawing">
      <rect x="10" y="22" width="20" height="4" fill="currentColor" />
      <rect x="18" y="16" width="4" height="6" fill="currentColor" />
      <circle cx="20" cy="10" r="8" fill="currentColor" />
    </svg>
  ),
  "Strobe 1500": () => (
    <svg viewBox="0 0 40 28" class="c-drawing">
      <rect x="4" y="8" width="32" height="12" fill="currentColor" />
    </svg>
  ),
};
const models = (names: readonly string[]): PickOption[] => names.map((name) => ({ name, ...(MODEL[name] ? { picture: MODEL[name] } : {}) }));

/** The pictures of a control's options, by its row. Not listed: its options have none. */
export const PICTURES: Record<string, (names: readonly string[]) => PickOption[]> = {
  "looks.step.gobo": gobos,
  "looks.step.colour.slot": slots,
  "looks.pattern.shape": patterns,
  "patch.panel.wiring": wirings,
  "patch.add.model": models,
};
