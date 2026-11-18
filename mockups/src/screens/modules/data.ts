// The modules from docs/design/modules.md, placed on the map. Controllers is new since review 2.

export interface Module {
  id: string;
  name: string;
  /** One short line under the name */
  sub?: string;
  /** One thing to know, on the card */
  note?: string;
  /** proposed, not rated yet */
  isNew?: boolean;
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
  { id: "engine", name: "Engine", sub: "commands in, state out", iface: ["send", "subscribe", "query"], hides: ["a listed set of commands", "transports: UI, MCP, OSC", "versions", "permissions"], ifaceSize: 3, hidesSize: 6, core: false, screens: ["console", "songs", "looks", "patch", "dmx", "inout"], x: 360, y: 150, h: 70 },
  { id: "mixer", name: "Mixer", note: "All who-wins rules live here, in one table. No screen and no other module decides between two values for the same light.", iface: ["apply", "frame"], hides: ["the who-wins rules, one table", "fades", "launch on beat", "flash", "release", "masters", "blackout"], ifaceSize: 2, hidesSize: 8, core: true, screens: ["console", "songs", "looks", "dmx"], x: 545, y: 165 },
  { id: "sources", name: "Sources", iface: ["evaluate"], hides: ["chases", "movement maths", "pixel matrices", "audio scripts", "palettes", "laser shapes"], ifaceSize: 1, hidesSize: 8, core: true, screens: [], x: 680, y: 270 },
  { id: "clock", name: "Clock", iface: ["now", "follow"], hides: ["tap", "audio beat", "MIDI clock", "DJ position", "timecode", "smoothing"], ifaceSize: 2, hidesSize: 6, core: true, screens: ["songs", "inout"], x: 545, y: 350 },
  { id: "rig", name: "Rig", iface: ["fixtures", "groups", "resolve"], hides: ["addresses", "modes", "16-bit", "colour mixing", "pan and tilt limits", "curves", "fixture swap"], ifaceSize: 3, hidesSize: 9, core: true, screens: ["patch", "dmx"], x: 680, y: 165 },
  { id: "library", name: "Fixture library", iface: ["search", "model"], hides: ["QLC+, OFL and GDTF file formats"], ifaceSize: 2, hidesSize: 5, core: false, screens: ["patch"], x: 545, y: 40, w: 170 },
  { id: "output", name: "Output", iface: ["devices", "send"], hides: ["USB timing", "reconnects", "Art-Net", "sACN", "DDP", "laser DAC"], ifaceSize: 2, hidesSize: 7, core: true, screens: ["dmx", "inout"], x: 815, y: 165, w: 110 },
  { id: "project", name: "Project", iface: ["read", "edit", "undo", "redo"], hides: ["file format", "autosave", "migration", "history"], ifaceSize: 4, hidesSize: 6, core: false, screens: ["looks"], x: 360, y: 40 },
  { id: "ctl", name: "Controllers", isNew: true, note: "Proposed in review 2: controller mapping lives with In/Out, like QLC+ profiles. Not rated yet.", iface: ["devices", "learn", "mappings"], hides: ["MIDI", "OSC", "controller profiles", "pad colours sent back"], ifaceSize: 3, hidesSize: 6, core: false, screens: ["inout"], x: 185, y: 190 },
  { id: "surface", name: "Surface", note: "Smaller since review 2: widgets and pages only. Hardware went to Controllers.", iface: ["layout", "press"], hides: ["widgets", "pages"], ifaceSize: 2, hidesSize: 4, core: false, screens: ["console"], x: 185, y: 250 },
  { id: "show", name: "Show", iface: ["record", "play", "edit"], hides: ["tracks", "clips", "nested song shows", "quantize", "section blocks"], ifaceSize: 3, hidesSize: 6, core: false, screens: ["songs"], x: 185, y: 310 },
  { id: "music", name: "Music", iface: ["playing", "analyse"], hides: ["VirtualDJ protocols", "analysis pipeline", "beat grids", "confidence"], ifaceSize: 2, hidesSize: 6, core: false, screens: ["songs"], x: 20, y: 370 },
  { id: "autopilot", name: "Autopilot", iface: ["start", "stop"], hides: ["choosing looks", "not repeating", "staying out of your hands' way"], ifaceSize: 2, hidesSize: 5, core: false, screens: ["songs"], x: 185, y: 370 },
];

/** Who issues commands: not modules, drawn quieter. */
export const WRITERS = [
  { id: "ui", name: "Web UI", x: 20, y: 30 },
  { id: "agents", name: "AI agents", x: 20, y: 100 },
  { id: "controllers", name: "Controllers", x: 20, y: 190 },
];

export const ARROWS: { id: string; d: string; state?: boolean }[] = [
  { id: "ui-engine", d: "M150,54 C260,54 300,165 360,170" },
  { id: "agents-engine", d: "M150,124 C250,124 300,182 360,182" },
  { id: "controllers-ctl", d: "M150,214 L185,214" },
  { id: "ctl-engine", d: "M315,214 C335,214 340,200 360,200" },
  { id: "surface-engine", d: "M315,276 C360,276 380,255 384,220" },
  { id: "show-engine", d: "M315,336 C400,336 412,280 414,220" },
  { id: "autopilot-engine", d: "M315,394 C440,394 455,280 450,220" },
  { id: "music-autopilot", d: "M150,394 L185,394" },
  { id: "music-clock", d: "M85,422 L85,432 L600,432 L600,402" },
  { id: "engine-mixer", d: "M490,185 L545,185" },
  { id: "engine-project", d: "M425,150 L425,92" },
  { id: "library-project", d: "M545,66 L490,66" },
  { id: "project-mixer", d: "M490,82 C540,82 575,120 580,165" },
  { id: "clock-mixer", d: "M600,350 L600,217" },
  { id: "sources-mixer", d: "M680,296 C650,296 640,260 632,217" },
  { id: "mixer-rig", d: "M655,191 L680,191" },
  { id: "rig-output", d: "M790,191 L815,191" },
  { id: "mixer-engine", d: "M545,207 C520,230 505,226 490,212", state: true },
];

/** One press, hop by hop: the path the dot takes, the module it lands on, what is said. */
export const TRACE: { path: string; at: string; say: string; reverse?: boolean }[] = [
  { path: "controllers-ctl", at: "ctl", say: "You press a pad on the Launchpad. Controllers looks up its mapping: Sunset." },
  { path: "ctl-engine", at: "engine", say: "send(start Sunset on Manual)" },
  { path: "engine-mixer", at: "mixer", say: "apply(command)" },
  { path: "mixer-rig", at: "rig", say: "frame(time) gives a look, 44 times a second" },
  { path: "rig-output", at: "output", say: "resolve(look) gives universes: bytes go out" },
  { path: "mixer-engine", at: "engine", say: "The new state comes back through the Engine" },
  { path: "ctl-engine", at: "ctl", say: "The pad colour is sent back: the Launchpad lights up", reverse: true },
];
