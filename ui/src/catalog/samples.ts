// The words and values the frames of the catalog are filled with, where the name of the row is not what
// stands on the control. Data. They are stand-ins: the catalog shows behaviour, the show file has the real ones.
import type { SituationId } from "../controls/situations.ts";
import type { Look } from "./look.ts";

type Sample = Partial<Look>;

const RED = { colour: "danger" } as const;

export const SAMPLES: Record<string, Sample> = {
  // ---- top bar
  "top.indicator.dj": { label: "DJ link", variant: "ready" },
  "top.indicator.recording": { label: "Recording", variant: "ready", ...RED },
  "top.indicator.autopilot": { label: "Autopilot", variant: "ready", colour: "autopilot" },
  "top.indicator.lock": { label: "Lock", variant: "ready" },
  "top.live": { label: "LIVE off" },
  "top.trouble": { label: "No lights connected", ...RED },
  "top.tap": { label: "Tap" },
  "top.tempo.source": { label: "from Tap", colour: "manual" },
  "how.walk.step": { label: "Start" },
  "modules.trace": { label: "Trace" },
  "top.blackout": { label: "Blackout", ...RED },
  "top.undo": { label: "Undo", sub: "Move widget" },
  "top.workspaces": { options: ["Console", "Songs", "Looks", "Patch", "DMX", "In/Out"] },
  "top.learn.cancel": { label: "Cancel" },
  "top.now-strip": { on: true },
  "top.redo": { label: "Redo", sub: "Move widget" },
  "top.show": { label: "Friday at the Loft" },
  "top.show.name": { label: "Name of the show", picked: "Friday at the Loft" },
  "top.show.new": { label: "New" },
  "top.show.open": { label: "Open…" },
  "top.show.save-as": { label: "Save as…" },
  // ---- Console
  "console.running.fold": { label: "Running", on: true, value: 3 },
  "console.running.release": { label: "Release", colour: "manual", on: true, sub: "Warm wash" },
  "console.running.stop": { label: "Stop", colour: "show", on: true, sub: "Chase L-R" },
  "console.running.release-all": { label: "Release all", colour: "manual" },
  "console.autopilot.on": { label: "Autopilot", colour: "autopilot", variant: "ready", sub: "Waits for LIVE" },
  "console.autopilot.style": { options: ["House", "Techno", "Chill"] },
  "console.pages": { options: ["Main", "Movers", "Pixels"] },
  "console.pad": { label: "Warm wash", variant: "idle" },
  "console.fader": { label: "Master" },
  "console.xy": { label: "Movers" },
  "console.speed": { picked: "normal" },
  "console.widget.frame": { label: "Pads", sub: "4 x 3" },
  "console.widget.resize": { label: "Corner of Pads" },
  "console.widget.remove": { label: "Remove" },
  "console.drawer.add": { label: "Pads", sub: "Pads that play looks" },
  "console.page.name": { label: "Name of the page", picked: "Movers" },
  "console.page.move": { label: "Page", sub: "2 of 3", options: ["Earlier", "Later"] },
  "console.widget.plays": { label: "Plays", options: ["Warm wash", "Chase L-R", "Strobe white"] },
  "console.widget.moves": { label: "Moves", options: ["Master", "Movers", "PARs", "Speed"] },
  "console.widget.words": { label: "Words", picked: "Front truss" },
  "console.cue.add": { label: "Add a look", options: ["Pick a look", "Warm wash", "Chase L-R", "Strobe white"] },
  "console.cue.move": { label: "Warm wash", sub: "Cue 2 of 4", options: ["Earlier", "Later"] },
  "console.cue.take-out": { label: "Take out", sub: "Warm wash" },
  // ---- Songs
  "songs.search": { label: "Search songs" },
  "songs.filter": { label: "Needs review" },
  "songs.song": { label: "Night Drive", sub: "124 BPM, 6 sections" },
  "songs.play-from": { label: "Play from Drop" },
  "songs.section": { label: "Drop", sub: "16 bars", colour: "show" },
  "songs.section.edge": { label: "Edge between Build and Drop" },
  "songs.recorded.mute": { label: "Mute recorded", colour: "show" },
  "songs.show.mute": { label: "Mute show", colour: "show" },
  "songs.section.name": { label: "Section name", picked: "Drop" },
  "songs.section.remove-look": { label: "Warm wash" },
  "songs.looks.add": { label: "Strobe white", sub: "Step" },
  // ---- Looks
  "looks.library.search": { label: "Search looks" },
  "looks.library.group.fold": { label: "Washes", value: 4, on: true },
  "looks.library.group.rename": { label: "Rename" },
  "looks.library.group.name": { label: "Group name", picked: "Washes" },
  "looks.library.group.delete": { label: "Delete group" },
  "looks.library.look": { label: "Warm wash", sub: "Step" },
  "looks.crumb": { label: "Chase L-R" },
  "looks.groups.remove": { label: "Washes" },
  "looks.groups.add": { label: "Add to a group", options: ["Washes", "Strobes", "Movers"] },
  "looks.try": { label: "Play on lights", variant: "idle" },
  "looks.connection": { label: "Chase L-R", sub: "Sequence" },
  "looks.step.fixtures": { label: "PAR 1-8", on: true },
  "looks.step.colour.remove": { label: "Take red out", variant: "remove" },
  "looks.step.colour.add": { label: "Add amber", variant: "add" },
  "looks.step.position": { label: "Position" },
  "looks.step.gobo": { options: ["Open", "Dots", "Star", "Bars"] },
  "looks.step.dimmer": { label: "Dimmer" },
  "looks.step.spin": { label: "Spin" },
  "looks.seq.item": { label: "Warm wash", sub: "1 beat" },
  "looks.seq.bar": { label: "Warm wash", sub: "At beat 1, 1 beat" },
  "looks.seq.bar.edge": { label: "End of Warm wash" },
  "looks.seq.width": { label: "Width", sub: "1 beat", options: ["Narrower", "Wider"] },
  "looks.seq.speed": { picked: "×1" },
  "looks.seq.spread": { label: "Spread" },
  "looks.seq.transition": { label: "Transition" },
  "looks.pattern.shape": { options: ["Plasma", "Stripes", "Wave", "Sparkle"] },
  "looks.pattern.measure": { label: "Measure", sub: "4 beats", options: ["÷2", "×2"] },
  "looks.pattern.colours": { label: "Colours from", options: ["Its own", "Warm wash", "Chase L-R"] },
  "looks.pattern.runs-on": { options: ["PARs", "Movers", "Panel"] },
  "looks.autopilot.style": { label: "House", sub: "5 rules" },
  "looks.autopilot.style-name": { label: "Style name", picked: "House" },
  "looks.autopilot.rule.when": { label: "When", options: ["A drop starts", "A build starts", "Every 8 bars"] },
  "looks.autopilot.rule.pick": { label: "Pick one from", options: ["Washes", "Strobes", "Movers"] },
  "looks.autopilot.rule.fit": { label: "Fit", options: ["To the beat", "To the bar", "Free"] },
  "looks.autopilot.lights": { label: "PAR 1-8", on: true },
  "looks.autopilot.wild": { label: "Wild" },
  "looks.autopilot.never-dark": { label: "Never dark", options: ["None", "Warm wash"] },
  "looks.name": { label: "Name", picked: "Warm wash" },
  "looks.copy": { label: "Copy" },
  "looks.seq.add": { label: "Add an item", options: ["Pick a look", "Warm wash", "Strobe white"] },
  "looks.seq.take-out": { label: "Take out", sub: "Warm wash" },
  "looks.step.colour.wheel": { label: "Colour wheel", value: 8, value2: 80 },
  // ---- Patch
  "patch.details.name": { label: "Name", picked: "PAR 3" },
  "patch.details.mode": { label: "Mode", options: ["7 channels", "4 channels", "16 channels"] },
  "patch.details.group": { label: "Group", options: ["PARs", "Movers", "Bars", "Laser"] },
  "patch.universes.search": { label: "Search universes" },
  "patch.universes.panel": { label: "Universes", value: 4 },
  "patch.universe": { label: "Universe 1", sub: "15 lights" },
  "patch.grid.panel": { label: "Panel", sub: "16 x 16" },
  "patch.grid.light": { label: "PAR 3", sub: "17 to 23" },
  "patch.fixtures.order": { label: "Order of PAR 3" },
  "patch.fixtures.light": { label: "PAR 3", sub: "Address 17, 7 channels" },
  "patch.fixtures.move": { label: "Order", sub: "3 of 8", options: ["Earlier", "Later"] },
  "patch.details.address": { label: "Address", sub: "17", options: ["Lower", "Higher"] },
  "patch.add.search": { label: "Find a light" },
  "patch.add.model": { options: ["LED PAR 64", "Moving head 250", "Strobe 1500"] },
  "patch.add.mode": { options: ["7 channels", "4 channels"] },
  "patch.add.address": { label: "Address", sub: "33", options: ["Lower", "Higher"] },
  "patch.panel.name": { label: "Name", picked: "Panel" },
  "patch.panel.size": { label: "Columns and rows", picked: "16 x 16" },
  "patch.panel.order": { label: "Colour order", options: ["RGB", "RBG", "GRB", "GBR", "BRG", "BGR"] },
  "patch.panel.start": { label: "Start universe", picked: "2" },
  // ---- DMX
  "dmx.universe": { options: ["1", "2", "3", "4"] },
  "dmx.channel": { label: "PAR 3 red", value: 128 },
  "dmx.channel.fader": { label: "Channel 17" },
  "dmx.held.channel": { label: "Channel 17", sub: "PAR 3 red, at 255" },
  "dmx.output.row": { label: "DMX USB Pro", sub: "Universe 1, running" },
  // ---- In/Out
  "inout.tempo.feed": { options: ["Tap", "Audio", "MIDI clock", "VirtualDJ", "Timecode"], picked: "VirtualDJ" },
  "inout.mapping.device": { label: "Device", options: ["APC mini", "Launchpad", "nanoKONTROL"] },
  "inout.mapping.message": { label: "Note or controller", picked: "Note 36" },
  "inout.out.kind": { label: "Kind", options: ["DMX USB Pro", "DDP", "Art-Net", "sACN"] },
  "inout.out.universes": { label: "Universes", picked: "2 to 9" },
  "inout.out.rate": { picked: "40" },
  "inout.audio.input": { label: "Listens to", options: ["Built-in input", "USB audio interface", "Loopback"] },
  "inout.osc.port": { label: "Port", picked: "9000" },
  // A toggle starts at its off state: its first press turns it on.
  "inout.out.test": { label: "Test" },
  "inout.osc.on": { label: "Off" },
  // ---- explainers
  "how.pad": { label: "Warm wash", variant: "idle" },
  "how.walk": { options: ["Press a pad", "Record and replay", "The autopilot yields"] },
  "modules.node": { label: "Playback", sub: "Module" },
  // ---- menu bar
  "menubar.item": { label: "Bossylights", variant: "mute", sub: "Running" },
};

/** Looks of states that are written down, where the rules of `look.ts` would guess. */
export const STATE_LOOKS: Record<string, Record<string, Sample>> = {
  // On while LIVE is on, red only while it writes into a song.
  // `song`: what the frame says, kept through every press. A lamp that is on with no song is armed.
  "top.indicator.recording": { armed: { on: true, colour: "plain", sub: "Armed", at: "on", song: false }, on: { on: true, variant: "on", song: true } },
  // Its lanes and decks, by `NOW`.
  "top.now-strip": {
    "something runs": { on: true, variant: "" },
    "nothing runs": { on: false, variant: "nothing" },
    "a song plays": { on: true, variant: "song" },
    "the autopilot plays": { on: true, variant: "autopilot" },
    "no song plays": { on: true, variant: "no song" },
  },
  "top.live": { off: { on: false, label: "LIVE off" }, on: { on: true, label: "LIVE on" } },
  "top.trouble": {
    "no lights": { label: "No lights connected" },
    "output lost": { label: "Output lost", sub: "DMX USB Pro" },
    "no engine": { label: "Engine not connected" },
  },
  "top.blackout": { off: { on: false }, held: { on: true, label: "Blackout on", sub: "While held" }, latched: { on: true, label: "Blackout on" } },
  "top.redo": { "nothing to redo": { off: "Nothing to redo", sub: "" }, "has a step": {} },
  "top.learn": { off: { on: false }, pick: { on: true, sub: "Pick a control" }, wait: { on: true, sub: "Move a control on the hardware" } },
  "top.tempo.source": { "from Tap": {}, "from the feed": { label: "from VirtualDJ", colour: "plain", off: "The feed drives the beat" } },
  // It waits for LIVE only while it is ready: on and off say nothing of it.
  "console.autopilot.on": { on: { on: true, variant: "on", sub: "" }, off: { on: false, variant: "off", sub: "" } },
  "console.running.release": { "pad on": { variant: "" }, "pad off, still held": { on: false, variant: "dashed", sub: "Warm wash, pad off" } },
  "console.edit-surface": { there: {}, "primary on an empty page": { colour: "paper", on: true } },
  "console.cue.go": { "not started": { sub: "Not started" }, "on a cue": { sub: "Cue 3 of 12" } },
  "console.cue.back": { "on a cue": { sub: "Cue 3 of 12" } },
  "console.xy": { free: { on: false }, held: { on: true } },
  "songs.song": {
    open: { on: true },
    playing: { variant: "Playing", colour: "paper" },
    "needs review": { variant: "Needs review" },
    queued: { variant: "Queued" },
  },
  "patch.universe": { shown: { on: true }, "has a clash": { variant: "Clash", colour: "danger" }, pixels: { variant: "Pixels" } },
  "patch.fixtures.light": { selected: { on: true }, clash: { variant: "Clash", colour: "danger" } },
  "looks.library.group.rename": { "library widened": {} },
  "looks.library.group.delete": { "library widened": {} },
  "looks.autopilot.style": { open: { on: true }, "in use": { variant: "In use", colour: "autopilot" } },
  "looks.connection": { "made of": { sub: "Step" }, "used by": { sub: "Console", label: "Main, pad 4" }, lights: { sub: "Lights", label: "PAR 1-8" } },
  "menubar.item": { "menu open": { on: true } },
};

/** The state a control is in when a situation is true, where that changes how it looks. */
export const SITUATION_STATE: Record<string, Partial<Record<SituationId, string>>> = {
  "top.live": { "live-on": "on", lock: "on", recording: "on" },
  "top.indicator.dj": { "live-on": "on", lock: "on", recording: "on", song: "on" },
  "top.indicator.recording": { "live-on": "armed", lock: "armed", recording: "on" },
  "top.indicator.autopilot": { "live-on": "on", lock: "on", recording: "on", autopilot: "on" },
  "top.indicator.lock": { "live-on": "on", lock: "on", recording: "on" },
  "console.autopilot.on": { "live-on": "on", lock: "on", recording: "on", autopilot: "on" },
  "top.trouble": { "no-engine": "no engine", empty: "no lights", "output-lost": "output lost" },
  "top.learn": { learn: "pick" },
  "top.now-strip": { empty: "nothing runs", song: "a song plays", recording: "a song plays", autopilot: "the autopilot plays", "no-song": "no song plays" },
  "console.running.release-all": { holding: "something held" },
  "console.pad": { holding: "pad off, still held", autopilot: "run by the autopilot", song: "run by the show" },
  "console.xy": { holding: "held" },
  "console.edit-surface": { empty: "primary on an empty page" },
  "dmx.universe": { "output-lost": "marked: trouble" },
  "menubar.item": { "live-on": "LIVE on", lock: "LIVE on", recording: "LIVE on", "no-engine": "not connected", empty: "trouble", "output-lost": "trouble" },
};

/** A deck of the DJ software as the Now strip shows it: its state in a word and the song, title and artist. */
export interface Deck {
  state: "Playing" | "Cued" | "Stopped";
  song: string;
}

/** What runs in the three lanes of the Now strip, and its two decks. Empty: nothing runs there. */
export interface Now {
  manual: string;
  show: string;
  autopilot: string;
  decks: readonly [Deck, Deck];
}

const STOPPED: Deck = { state: "Stopped", song: "" };
const CUED: Deck = { state: "Cued", song: "Sunrise, Mira" };

/** The usual state has no song: no deck plays and the Show lane is empty. Every other is the usual plus one fact. */
const USUAL_NOW: Now = { manual: "Warm wash", show: "", autopilot: "", decks: [STOPPED, CUED] };

/** The Now strip by the variant of its look. */
export const NOW: Record<string, Now> = {
  "": USUAL_NOW,
  nothing: { manual: "", show: "", autopilot: "", decks: [STOPPED, STOPPED] },
  song: { ...USUAL_NOW, show: "Drop", decks: [{ state: "Playing", song: "Night drive, Kaito" }, CUED] },
  autopilot: { ...USUAL_NOW, autopilot: "Slow sweep" },
  "no song": { ...USUAL_NOW, decks: [STOPPED, STOPPED] },
};
