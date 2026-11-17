// The fixture library the guided add searches. Sample data, no real brands.
import type { Fixture } from "../../fake/store";

export interface Mode {
  name: string;
  channels: number;
  /** What the mode gives you, in plain words */
  about: string;
}

export interface Model {
  id: string;
  name: string;
  kind: Fixture["kind"];
  /** What kind of light it is, in plain words */
  type: string;
  modes: Mode[];
}

export const LIBRARY: Model[] = [
  {
    id: "flat-par",
    name: "Flat PAR RGBW",
    kind: "par",
    type: "Colour wash",
    modes: [
      { name: "4 channel", channels: 4, about: "Red, green, blue, white" },
      { name: "7 channel", channels: 7, about: "Dimmer, colour, strobe, programs" },
    ],
  },
  {
    id: "mini-blinder",
    name: "Mini Blinder 2",
    kind: "par",
    type: "Blinder",
    modes: [
      { name: "1 channel", channels: 1, about: "Both lamps together" },
      { name: "2 channel", channels: 2, about: "Each lamp on its own" },
    ],
  },
  {
    id: "spot-60",
    name: "Spot 60 LED",
    kind: "mover",
    type: "Moving head, spot",
    modes: [
      { name: "11 channel", channels: 11, about: "Position, colour wheel, gobo" },
      { name: "16 channel", channels: 16, about: "Fine position, prism, focus" },
    ],
  },
  {
    id: "wash-beam",
    name: "Wash Beam 7",
    kind: "mover",
    type: "Moving head, wash",
    modes: [
      { name: "13 channel", channels: 13, about: "Position, colour, zoom" },
      { name: "16 channel", channels: 16, about: "Each ring on its own" },
    ],
  },
  {
    id: "pixel-bar",
    name: "Pixel Bar 8",
    kind: "bar",
    type: "Pixel bar",
    modes: [
      { name: "3 channel", channels: 3, about: "One colour for the whole bar" },
      { name: "24 channel", channels: 24, about: "Each of the 8 pixels on its own" },
    ],
  },
  {
    id: "show-laser",
    name: "Show laser RGB",
    kind: "laser",
    type: "Laser",
    modes: [
      { name: "7 channel", channels: 7, about: "Patterns and colour" },
      { name: "10 channel", channels: 10, about: "Patterns, colour, position, speed" },
    ],
  },
];

const GROUP: Record<Fixture["kind"], Fixture["group"]> = { par: "pars", mover: "movers", bar: "bars", laser: "laser" };
const NAME: Record<Fixture["kind"], string> = { par: "PAR", mover: "Mover", bar: "Bar", laser: "Laser" };

/** A new fixture from a model and mode, named after the ones already patched. */
export function newFixture(rig: Fixture[], model: Model, mode: Mode, address: number): Fixture {
  const same = rig.filter((f) => f.kind === model.kind).length;
  const name = `${NAME[model.kind]} ${same + 1}`;
  return {
    id: `${model.kind}-${Date.now().toString(36)}`,
    name,
    group: GROUP[model.kind],
    kind: model.kind,
    x: 0.5,
    y: 0.42,
    model: model.name,
    mode: mode.name,
    universe: 1,
    address,
    channels: mode.channels,
  };
}
