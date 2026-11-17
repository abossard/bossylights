// The twelve modules from docs/design/modules.md, placed on the map.

export interface Module {
  id: string;
  name: string;
  /** The whole interface */
  iface: string[];
  hides: string[];
  /** Depth from the table: interface size and what it hides */
  ifaceSize: number;
  hidesSize: number;
  core: boolean;
  /** Screens that use it, by screen id */
  screens: string[];
  x: number;
  y: number;
  w?: number;
  h?: number;
}

export const MODULES: Module[] = [
  { id: "door", name: "Door", iface: ["send", "subscribe", "query"], hides: ["transports: UI, MCP, OSC", "versions", "permissions"], ifaceSize: 3, hidesSize: 6, core: false, screens: ["console", "show", "songs", "looks", "patch", "dmx", "inout"], x: 360, y: 150, h: 70 },
  { id: "mixer", name: "Mixer", iface: ["apply", "frame"], hides: ["layers", "fades", "launch on beat", "flash", "release", "masters", "blackout"], ifaceSize: 2, hidesSize: 8, core: true, screens: ["console", "show", "looks", "dmx"], x: 545, y: 165 },
  { id: "sources", name: "Sources", iface: ["evaluate"], hides: ["chases", "movement maths", "pixel matrices", "audio scripts", "palettes", "laser shapes"], ifaceSize: 1, hidesSize: 8, core: true, screens: [], x: 680, y: 270 },
  { id: "clock", name: "Clock", iface: ["now", "follow"], hides: ["tap", "audio beat", "MIDI clock", "DJ position", "timecode", "smoothing"], ifaceSize: 2, hidesSize: 6, core: true, screens: ["show", "inout"], x: 545, y: 350 },
  { id: "rig", name: "Rig", iface: ["fixtures", "groups", "resolve"], hides: ["addresses", "modes", "16-bit", "colour mixing", "pan and tilt limits", "curves", "fixture swap"], ifaceSize: 3, hidesSize: 9, core: true, screens: ["patch", "dmx"], x: 680, y: 165 },
  { id: "library", name: "Fixture library", iface: ["search", "model"], hides: ["QLC+, OFL and GDTF file formats"], ifaceSize: 2, hidesSize: 5, core: false, screens: ["patch"], x: 545, y: 40, w: 170 },
  { id: "output", name: "Output", iface: ["devices", "send"], hides: ["USB timing", "reconnects", "Art-Net", "sACN", "DDP", "laser DAC"], ifaceSize: 2, hidesSize: 7, core: true, screens: ["dmx", "inout"], x: 815, y: 165, w: 110 },
  { id: "project", name: "Project", iface: ["read", "edit", "undo", "redo"], hides: ["file format", "autosave", "migration", "history"], ifaceSize: 4, hidesSize: 6, core: false, screens: ["looks"], x: 360, y: 40 },
  { id: "surface", name: "Surface", iface: ["layout", "press", "reflect"], hides: ["widgets", "pages", "MIDI learn", "controller profiles", "pad colours"], ifaceSize: 3, hidesSize: 7, core: false, screens: ["console", "inout"], x: 185, y: 190 },
  { id: "show", name: "Show", iface: ["record", "play", "edit"], hides: ["tracks", "clips", "nested song shows", "quantize", "section blocks"], ifaceSize: 3, hidesSize: 6, core: false, screens: ["show", "songs"], x: 185, y: 290 },
  { id: "music", name: "Music", iface: ["playing", "analyse"], hides: ["VirtualDJ protocols", "analysis pipeline", "beat grids", "confidence"], ifaceSize: 2, hidesSize: 6, core: false, screens: ["show", "songs"], x: 20, y: 370 },
  { id: "autopilot", name: "Autopilot", iface: ["start", "stop"], hides: ["choosing looks", "not repeating", "staying out of your hands' way"], ifaceSize: 2, hidesSize: 5, core: false, screens: ["show"], x: 185, y: 370 },
];

/** Who issues commands: not modules, drawn quieter. */
export const WRITERS = [
  { id: "ui", name: "Web UI", x: 20, y: 30 },
  { id: "agents", name: "AI agents", x: 20, y: 100 },
  { id: "controllers", name: "Controllers", x: 20, y: 190 },
];

export const ARROWS: { id: string; d: string; state?: boolean }[] = [
  { id: "ui-door", d: "M150,54 C260,54 300,165 360,170" },
  { id: "agents-door", d: "M150,124 C250,124 300,182 360,182" },
  { id: "controllers-surface", d: "M150,214 L185,214" },
  { id: "surface-door", d: "M315,214 C335,214 340,200 360,200" },
  { id: "show-door", d: "M315,314 C390,314 400,270 405,220" },
  { id: "autopilot-door", d: "M315,394 C440,394 455,280 450,220" },
  { id: "music-autopilot", d: "M150,394 L185,394" },
  { id: "music-clock", d: "M85,422 L85,432 L600,432 L600,402" },
  { id: "door-mixer", d: "M490,185 L545,185" },
  { id: "door-project", d: "M425,150 L425,92" },
  { id: "library-project", d: "M545,66 L490,66" },
  { id: "project-mixer", d: "M490,82 C540,82 575,120 580,165" },
  { id: "clock-mixer", d: "M600,350 L600,217" },
  { id: "sources-mixer", d: "M680,296 C650,296 640,260 632,217" },
  { id: "mixer-rig", d: "M655,191 L680,191" },
  { id: "rig-output", d: "M790,191 L815,191" },
  { id: "mixer-door", d: "M545,207 C520,230 505,226 490,212", state: true },
];

/** One press, hop by hop: the path the dot takes, the module it lands on, what is said. */
export const TRACE: { path: string; at: string; say: string; reverse?: boolean }[] = [
  { path: "controllers-surface", at: "surface", say: "You press Sunset. Surface turns the press into a command." },
  { path: "surface-door", at: "door", say: "send(start Sunset on Manual)" },
  { path: "door-mixer", at: "mixer", say: "apply(command)" },
  { path: "mixer-rig", at: "rig", say: "frame(time) gives a look, 44 times a second" },
  { path: "rig-output", at: "output", say: "resolve(look) gives universes: bytes go out" },
  { path: "mixer-door", at: "door", say: "The new state comes back through the Door" },
  { path: "surface-door", at: "surface", say: "reflect(state): the pad lights up", reverse: true },
];
