// Fake data shared by every screen. Nothing here talks to an engine.

export type Layer = "manual" | "show" | "autopilot";
export type GroupId = "movers" | "pars" | "bars" | "laser";
export type LookKind = "wash" | "movement" | "strobe" | "pixels" | "laser";
export type SectionLabel = "intro" | "build" | "drop" | "break" | "outro";

export interface Fixture {
  id: string;
  name: string;
  group: GroupId;
  kind: "mover" | "par" | "bar" | "laser";
  /** position on the stage, 0..1 from the left and from the back */
  x: number;
  y: number;
  model: string;
  mode: string;
  universe: number;
  address: number;
  channels: number;
}

export interface Look {
  id: string;
  name: string;
  /** one colour, or several for a multi-colour chip */
  colors: string[];
  groups: GroupId[];
  kind: LookKind;
}

export interface Section {
  label: SectionLabel;
  title: string;
  /** seconds */
  length: number;
  /** 0..1 */
  energy: number;
  looks: string[];
}

export interface Song {
  id: string;
  title: string;
  artist: string;
  bpm: number;
  /** 0..1, how sure the analysis is */
  confidence: number;
  analysis: "analysed" | "queued" | "needs-review";
  sections: Section[];
}

export const SHOW_NAME = "Friday at the Garage";
export const TEMPO = 126;
export const MASTER = 85;

/** The fake show clock starts here (22:47:12), in seconds since midnight. */
export const CLOCK_START = 22 * 3600 + 47 * 60 + 12;

export const GROUPS: { id: GroupId; name: string }[] = [
  { id: "movers", name: "Movers" },
  { id: "pars", name: "PAR 1-8" },
  { id: "bars", name: "Bars" },
  { id: "laser", name: "Laser" },
];

const mover = (n: string, x: number, address: number): Fixture => ({
  id: n.toLowerCase().replace(/\s+/g, "-"),
  name: n,
  group: "movers",
  kind: "mover",
  x,
  y: 0.14,
  model: "Spot 60 LED",
  mode: "16 channel",
  universe: 1,
  address,
  channels: 16,
});

const par = (i: number): Fixture => ({
  id: `par-${i}`,
  name: `PAR ${i}`,
  group: "pars",
  kind: "par",
  x: 0.16 + (i - 1) * (0.68 / 7),
  y: 0.6,
  model: "Flat PAR RGBW",
  mode: "7 channel",
  universe: 1,
  address: 65 + (i - 1) * 7,
  channels: 7,
});

export const RIG: Fixture[] = [
  mover("Mover L1", 0.22, 1),
  mover("Mover L2", 0.36, 17),
  {
    id: "laser",
    name: "Laser",
    group: "laser",
    kind: "laser",
    x: 0.5,
    y: 0.14,
    model: "Show laser RGB",
    mode: "10 channel",
    universe: 1,
    // overlaps Bar R on purpose: the one address conflict in the rig
    address: 161,
    channels: 10,
  },
  mover("Mover R1", 0.64, 33),
  mover("Mover R2", 0.78, 49),
  ...[1, 2, 3, 4, 5, 6, 7, 8].map(par),
  {
    id: "bar-l",
    name: "Bar L",
    group: "bars",
    kind: "bar",
    x: 0.05,
    y: 0.42,
    model: "Pixel Bar 8",
    mode: "24 channel",
    universe: 1,
    address: 121,
    channels: 24,
  },
  {
    id: "bar-r",
    name: "Bar R",
    group: "bars",
    kind: "bar",
    x: 0.95,
    y: 0.42,
    model: "Pixel Bar 8",
    mode: "24 channel",
    universe: 1,
    address: 145,
    channels: 24,
  },
];

const look = (
  id: string,
  name: string,
  colors: string | string[],
  groups: GroupId[],
  kind: LookKind,
): Look => ({ id, name, colors: Array.isArray(colors) ? colors : [colors], groups, kind });

export const LOOKS: Look[] = [
  look("warm-wash", "Warm wash", "#ffb46b", ["pars", "bars"], "wash"),
  look("cold-wash", "Cold wash", "#bfe3ff", ["pars", "bars"], "wash"),
  look("sunset", "Sunset", "#ff7a3d", ["movers", "pars"], "wash"),
  look("deep-blue", "Deep blue", "#2a4bff", ["pars", "bars"], "wash"),
  look("uv", "UV", "#7b2cff", ["pars"], "wash"),
  look("red-alert", "Red alert", "#ff2a2a", ["pars", "bars", "movers"], "wash"),
  look("pulse", "Pulse", "#ef3e7b", ["pars"], "wash"),
  look("breathe", "Breathe", "#ffd9a8", ["pars", "bars"], "wash"),
  look("chase-lr", "Chase L-R", "#20b2c9", ["bars"], "pixels"),
  look("chase-ping", "Chase ping", "#9ef0ff", ["bars"], "pixels"),
  look("sparkle", "Sparkle", "#fffbe0", ["bars"], "pixels"),
  look("strobe", "Strobe", "#ffffff", ["pars"], "strobe"),
  look("blinder", "Blinder", "#fff1b0", ["pars"], "strobe"),
  look("fan-out", "Fan out", "#f2b705", ["movers"], "movement"),
  look("circle", "Circle", "#3ddc97", ["movers"], "movement"),
  look("tilt-wave", "Tilt wave", "#6fa8ff", ["movers"], "movement"),
  look("center", "Center", "#f4efe6", ["movers"], "movement"),
  look("crowd-sweep", "Crowd sweep", "#ff9ad0", ["movers"], "movement"),
  look("rainbow", "Rainbow", ["#ef3e7b", "#f2b705", "#20b2c9"], ["bars"], "pixels"),
  look("fire", "Fire", "#ff5a1f", ["bars", "pars"], "pixels"),
  look("ice", "Ice", "#8fe8ff", ["pars", "bars"], "wash"),
  look("blackout-hold", "Blackout hold", "#000000", ["movers", "pars", "bars", "laser"], "strobe"),
  look("flash-white", "Flash white", "#ffffff", ["movers", "pars", "bars"], "strobe"),
  look("laser-fan", "Laser fan", "#39ff6a", ["laser"], "laser"),
  look("amber-glow", "Amber glow", "#ffa000", ["pars"], "wash"),
  look("magenta-pop", "Magenta pop", "#ff2fd0", ["pars"], "wash"),
  look("teal-drift", "Teal drift", "#12c2a8", ["bars"], "pixels"),
  look("bass-hit", "Bass hit", "#ff4d6d", ["pars"], "strobe"),
  look("dj-spot", "DJ spot", "#fff4dc", ["movers"], "movement"),
  look("slow-fade", "Slow fade", "#c9a27a", ["pars", "bars"], "wash"),
  look("lime-zap", "Lime zap", "#b6ff2e", ["bars"], "pixels"),
  look("gobo-spin", "Gobo spin", "#d7b8ff", ["movers"], "movement"),
];

export const lookById = (id: string): Look => {
  const found = LOOKS.find((l) => l.id === id);
  if (!found) throw new Error(`unknown look ${id}`);
  return found;
};

/** Console pages: 32 pads each, by look id. */
export const PAD_PAGES: Record<string, string[]> = {
  main: LOOKS.map((l) => l.id),
  movers: [
    "fan-out", "circle", "tilt-wave", "center", "crowd-sweep", "dj-spot", "gobo-spin", "sunset",
    "red-alert", "flash-white", "blackout-hold", "warm-wash", "cold-wash", "deep-blue", "uv", "strobe",
  ],
  pixels: [
    "chase-lr", "chase-ping", "sparkle", "rainbow", "fire", "teal-drift", "lime-zap", "ice",
    "warm-wash", "deep-blue", "slow-fade", "breathe", "strobe", "blinder", "flash-white", "blackout-hold",
  ],
};

const min = 60;

const sections = (
  parts: [SectionLabel, string, number, number, string[]][],
): Section[] =>
  parts.map(([label, title, length, energy, looks]) => ({ label, title, length, energy, looks }));

/** Sample data: made-up titles and artists. */
export const SONGS: Song[] = [
  {
    id: "neon-tide",
    title: "Neon Tide",
    artist: "Kessa Marr",
    bpm: 124,
    confidence: 0.93,
    analysis: "analysed",
    sections: sections([
      ["intro", "Intro", 4 * min, 0.25, ["breathe"]],
      ["build", "Build-up", 5 * min, 0.55, ["warm-wash", "tilt-wave"]],
      ["drop", "Drop", 6 * min, 0.9, ["pulse", "circle", "chase-lr"]],
      ["break", "Break", 3 * min, 0.35, ["deep-blue"]],
      ["drop", "Drop", 4 * min, 0.85, ["bass-hit", "fan-out"]],
      ["outro", "Outro", 3 * min, 0.3, ["slow-fade"]],
    ]),
  },
  {
    id: "glass-hours",
    title: "Glass Hours",
    artist: "Oto Vance",
    bpm: 126,
    confidence: 0.97,
    analysis: "analysed",
    sections: sections([
      ["intro", "Intro", 4 * min, 0.3, ["breathe"]],
      ["build", "Build-up", 5 * min, 0.6, ["sunset", "chase-lr"]],
      ["drop", "Drop", 4 * min, 0.95, ["pulse", "fan-out", "sparkle"]],
      ["break", "Break", 3 * min, 0.4, ["deep-blue", "tilt-wave"]],
      ["drop", "Drop", 4 * min, 0.9, ["bass-hit", "circle", "chase-ping"]],
      ["outro", "Outro", 5 * min, 0.3, ["slow-fade"]],
    ]),
  },
  {
    id: "lowlight-parade",
    title: "Lowlight Parade",
    artist: "Ruin and Ivy",
    bpm: 128,
    confidence: 0.81,
    analysis: "analysed",
    sections: sections([
      ["intro", "Intro", 3 * min, 0.3, ["cold-wash"]],
      ["build", "Build-up", 6 * min, 0.6, ["ice", "crowd-sweep"]],
      ["drop", "Drop", 5 * min, 0.9, []],
      ["break", "Break", 4 * min, 0.4, ["uv"]],
      ["drop", "Drop", 4 * min, 0.92, ["magenta-pop", "gobo-spin", "rainbow"]],
      ["outro", "Outro", 3 * min, 0.25, ["amber-glow"]],
    ]),
  },
  {
    id: "paper-satellites",
    title: "Paper Satellites",
    artist: "Dali Okon",
    bpm: 122,
    confidence: 0,
    analysis: "queued",
    sections: sections([
      ["intro", "Intro", 5 * min, 0.3, ["warm-wash"]],
      ["build", "Build-up", 5 * min, 0.55, ["fire", "fan-out"]],
      ["drop", "Drop", 5 * min, 0.85, ["lime-zap", "circle", "pulse"]],
      ["break", "Break", 4 * min, 0.35, ["teal-drift"]],
      ["outro", "Outro", 6 * min, 0.25, ["slow-fade"]],
    ]),
  },
];

/** In the library, not in tonight's set. Sample data. */
export const LIBRARY_ONLY: Song[] = [
  {
    id: "static-bloom",
    title: "Static Bloom",
    artist: "Mira Quell",
    bpm: 118,
    confidence: 0.42,
    analysis: "needs-review",
    sections: sections([
      ["intro", "Intro", 3 * min, 0.3, []],
      ["build", "Build-up", 4 * min, 0.5, []],
      ["drop", "Drop", 4 * min, 0.8, []],
      ["outro", "Outro", 3 * min, 0.3, []],
    ]),
  },
];

/** The set starts at 22:15. Song 2 starts at 22:40 and is playing when the mockup opens. */
export const SET_START = 22 * 3600 + 15 * 60;
