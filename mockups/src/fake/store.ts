// One fake state store drives every screen. A pad pressed on the Console shows up
// everywhere that reads this store.
import { createEffect, createRoot, createSignal } from "solid-js";
import { createStore, produce, reconcile } from "solid-js/store";
import { GOBOS, initialLibrary, type Entry, type Item, type Pattern, type Sequence, type Step } from "./library";
import {
  CLOCK_START,
  GROUPS,
  LIBRARY_ONLY,
  LOOKS,
  MASTER,
  RIG,
  SET_START,
  SHOW_NAME,
  SONGS,
  TEMPO,
  lookById,
  PAD_PAGES,
  setLookResolver,
  type Fixture,
  type GroupId,
  type Layer,
  type Look,
  type Section,
  type SectionLabel,
  type Song,
} from "./data";

export * from "./data";
export * from "./library";

/** A control mapped with Learn: something on screen, played from a hardware control. */
export interface Mapping {
  id: string;
  /** what it does, as the button or value is called on screen */
  control: string;
  /** where that control is: a screen title, or "Top bar" */
  screen: string;
  device: string;
  /** the MIDI message, in words: "Note 91", "CC 21" */
  message: string;
  /** the controller lights up to show the state */
  feedback: boolean;
}

/** An LED pixel panel: it spreads over as many universes as it needs. */
export interface PixelPanel {
  id: string;
  name: string;
  cols: number;
  rows: number;
  /** snake: every other row runs back. zigzag: every row starts on the same side */
  wiring: "snake" | "zigzag";
  startUniverse: number;
  /** channels per pixel: RGB 3, RGBW 4 */
  pixel: "RGB" | "RGBW";
  /** the order the colours are wired in, as in QLC+ */
  order: ColourOrder;
}
export const COLOUR_ORDERS = ["RGB", "BGR", "BRG", "GBR", "GRB", "RBG"] as const;
export type ColourOrder = (typeof COLOUR_ORDERS)[number];

/** Where a panel sits on one universe. */
export interface PanelRun {
  panel: PixelPanel;
  universe: number;
  address: number;
  channels: number;
  firstPixel: number;
  lastPixel: number;
}

/** Where the beat comes from. */
export type ClockFeed = "tap" | "audio" | "midi" | "vdj" | "timecode";

/** A device that sends light data out. */
export interface Output {
  id: string;
  universe: number | null;
  device: string;
  kind: "USB DMX" | "Art-Net" | "sACN" | "DDP";
  state: "connected" | "reconnecting" | "error";
  /** frames per second actually sent */
  fps: number;
  errors: number;
  detail: string;
}

export type FaderId = "wash" | "movers" | "pixels" | "strobeRate" | "laser";
/** How a pick lines up with the music. */
export type Fit = "loop" | "ends" | "once";
export const FIT_NAME: Record<Fit, string> = { loop: "Loops until the section ends", ends: "A full pass ends on the next section", once: "Plays once, then holds" };
/** One autopilot rule: when (song section), pick (one look from a group), fit. */
export interface ApRule {
  id: string;
  when: SectionLabel;
  pick: string;
  fit: Fit;
}
/** A style: a named set of rules. */
export interface ApStyle {
  id: string;
  name: string;
  rules: ApRule[];
}
export const SECTION_NAME: Record<SectionLabel, string> = { intro: "Intro", build: "Build-up", drop: "Drop", break: "Break", outro: "Outro" };

/** One look that runs right now, for the Console's Running panel. */
export interface Running {
  id: string;
  rank: Layer;
  lookId: string;
  name: string;
  colors: string[];
  /** manual only: the pad is off, it is still held */
  padOff?: boolean;
}

/** A jump from one screen to a place on another (the Connections diagram). */
export interface Jump {
  consolePage?: string;
  song?: string;
  section?: number;
  fixtureGroup?: GroupId;
  autopilotRule?: string;
  look?: string;
}

/** The four things the LIVE switch turns on, each of which can be turned off on its own. */
export type Indicator = "dj" | "record" | "autopilot" | "lock";

export interface ManualBlock {
  id: number;
  lookId: string;
  start: number;
  /** null while it runs */
  end: number | null;
  /** flash: only while a finger is down. held: until stopped or released. */
  mode: "flash" | "held";
  /** When manual control of what it touched ended: on Release, or with the pad if that is the rule. null: still held. */
  released: number | null;
}

/** What is decided on its own, for each group of lights. */
export type Prop = "colour" | "brightness" | "position";
export const PROPS: Prop[] = ["colour", "brightness", "position"];

/** The who-wins settings. They live in one place: the "Who wins" panel on the DMX screen. */
export interface Rules {
  /** rule 3, brightness among equals */
  brightness: "latest" | "highest";
  /** rule 4, when manual control ends */
  manualEnds: "release" | "pad";
  /** rule 5, when a show or autopilot look ends */
  lookEnds: "fallback" | "stay";
}

/** Who has one property of one group right now. */
export interface Winner {
  rank: Layer;
  look: Look;
  /** manual: the pad is off, the Console still holds it */
  padOff?: boolean;
  /** show or autopilot: the look has ended and stays as it was left */
  left?: boolean;
}

/** One thing the Console holds until Release. */
export interface Hold {
  id: string;
  name: string;
  since: number;
  /** false: the pad was switched off, what it touched is still held */
  live: boolean;
  props: Prop[];
  groups: GroupId[];
  /** where an equal has it instead: "brightness: Blinder has it" */
  beaten: string[];
}

export interface ManualEvent {
  at: number;
  lookId: string;
  action: "start" | "stop" | "flash";
}

export type ClipTrack = "looks" | "movers" | "pixels" | "laser" | "recorded";

/** A clip on the Show timeline. Kept clips came from an autopilot pick. */
export interface ShowClip {
  id: number;
  track: ClipTrack;
  lookId: string;
  start: number;
  end: number;
  origin: "programmed" | "kept" | "recorded";
}

/** A VirtualDJ deck. Position and length in seconds. */
export interface Deck {
  number: "1" | "2";
  /** null: nothing loaded on this deck */
  song: Song | null;
  playing: boolean;
  position: number;
  length: number;
}

/** A song in tonight's set, placed on the clock. */
export interface SetEntry {
  song: Song;
  index: number;
  start: number;
  end: number;
  mark: "recorded" | "show" | "auto" | "not analysed";
}

/** A section of a song placed on the clock. */
export interface PlacedSection {
  key: string;
  song: Song;
  songIndex: number;
  section: SongSection;
  index: number;
  start: number;
  end: number;
  songStart: number;
}

/** What the timetable and the Now strip draw. Times are show-clock seconds. */
export interface LaneBlock {
  id: string;
  title: string;
  sub?: string;
  start: number;
  end: number | null;
  /** Label on the open tail of a block that is still running: "holding" or "flash". */
  tail?: string;
}

/** A section of a song, with an id that survives moving, splitting and merging. */
export type SongSection = Section & { id: string };

/** One thing written down while recording, in seconds from the start of the song. */
export interface RecEvent {
  at: number;
  kind: "start" | "stop" | "flash" | "fader";
  lookId?: string;
  /** flash length in seconds */
  dur?: number;
  fader?: string;
  value?: number;
}

/** One recording pass. */
export interface Pass {
  id: string;
  /** show-clock time it started */
  started: number;
  events: RecEvent[];
}
/** What was recorded for a song: one row. Every pass adds to it. */
export interface Recorded {
  muted: boolean;
  /** the last pass is still being written: the row is red */
  writing: boolean;
  passes: Pass[];
}

export interface Lane {
  /** which of the three rows */
  layer: Layer;
  title: string;
  /** true while the lane has something running: the state chip is filled */
  on: boolean;
  /** One word for the lane's state: "holding", "playing", "idle", "waiting", "off". */
  chip: string;
  /** A short line saying more: what it covers, what plays, what comes next. */
  status: string;
  blocks: LaneBlock[];
  empty?: string;
}

interface UndoEntry {
  label: string;
  undo: () => void;
  key?: string;
  at: number;
}

interface State {
  showName: string;
  tempo: number;
  master: number;
  /** The LIVE switch: the night is running. Lights follow the app either way. */
  night: { live: boolean; indicators: Record<Indicator, boolean> };
  lightsConnected: boolean;
  /** The patch: every fixture with its place and address. */
  rig: Fixture[];
  /** A fixture flashing so you can find it in the room (Patch). */
  identify: string | null;
  /** Looks per song section set in the Song Director, by "songId:index". */
  sectionLooks: Record<string, string[]>;
  /** Songs whose analysis someone checked by hand. */
  reviewed: string[];
  /** Channels held by hand on the DMX screen, universe 1: address -> 0..255. */
  held: Record<number, number>;
  clockFeed: ClockFeed;
  /** Pixel panels, each over several universes. */
  panels: PixelPanel[];
  /** Everything mapped with Learn, anywhere in the app. */
  mappings: Mapping[];
  /** Steps, sequences and patterns, by id. */
  library: Record<string, Item>;
  /** The Console's pad banks, by page. */
  pads: Record<string, string[]>;
  /** Looks the show replays right now from a recording (How it works). */
  replay: string[];
  /** false: the cued deck has nothing loaded (an edge state to look at) */
  cuedDeckLoaded: boolean;
  blackout: boolean;
  blackoutHold: boolean;
  faders: Record<FaderId, number>;
  xy: { pan: number; tilt: number };
  manual: ManualBlock[];
  /** The Movers position pad was touched: the Console holds the position since then. */
  xyHeld: number | null;
  /** The who-wins settings. */
  rules: Rules;
  /** Sections per song, editable. */
  songSections: Record<string, SongSection[]>;
  /** What was recorded per song: one row, every pass adds to it. */
  recorded: Record<string, Recorded>;
  /** Looks of the show or the autopilot stopped by hand, by placed section key. */
  stopped: Record<string, string[]>;
  /** The groups of the looks library, in their order. A group can be empty. */
  lookGroups: string[];
  /** Where a jump from another screen wants to land. */
  jump: Jump;
  /** A song's show, muted. */
  showMuted: Record<string, boolean>;
  autopilot: {
    on: boolean;
    /** id of the style in use */
    style: string;
    styles: ApStyle[];
    wild: number;
    /** fixture groups it may use */
    groups: Record<GroupId, boolean>;
    /** what plays when nothing else does: a look id, or null for dark */
    neverDark: string | null;
    /** looks replaced with "skip this look", by placed section key */
    swaps: Record<string, string[]>;
    likes: string[];
  };
  events: ManualEvent[];
  history: UndoEntry[];
}

const COALESCE_MS = 1200;

function build() {
  // ---- clock ---------------------------------------------------------
  const t0 = performance.now();
  const [elapsed, setElapsed] = createSignal(0);
  setInterval(() => setElapsed(performance.now() - t0), 50);

  const [state, set] = createStore<State>({
    showName: SHOW_NAME,
    tempo: TEMPO,
    master: MASTER,
    night: { live: true, indicators: { dj: true, record: true, autopilot: true, lock: true } },
    lightsConnected: true,
    rig: RIG.map((f) => ({ ...f })),
    identify: null,
    sectionLooks: Object.fromEntries([
      ...SONGS[0].sections.map((x, i) => [`${SONGS[0].id}-${i}`, x.looks]),
      ...SONGS[1].sections.map((x, i) => [`${SONGS[1].id}-${i}`, i === 1 ? [] : x.looks]),
    ]),
    reviewed: [],
    // three channels held by hand: PAR 1 red, PAR 5 dimmer, Mover R1 pan
    held: { 66: 255, 93: 180, 33: 96 },
    clockFeed: "vdj",
    panels: [{ id: "panel-1", name: "Pixel panel", cols: 24, rows: 16, wiring: "snake", startUniverse: 2, pixel: "RGB", order: "GRB" }],
    mappings: [
      { id: "m1", control: "Blackout", screen: "Top bar", device: "Launchpad Mini", message: "Note 91", feedback: true },
      { id: "m2", control: "Master", screen: "Console", device: "Launchpad Mini", message: "CC 21", feedback: false },
    ],
    replay: [],
    library: initialLibrary(),
    pads: JSON.parse(JSON.stringify(PAD_PAGES)) as Record<string, string[]>,
    cuedDeckLoaded: true,
    blackout: false,
    blackoutHold: false,
    faders: { wash: 70, movers: 60, pixels: 45, strobeRate: 30, laser: 0 },
    xy: { pan: 38, tilt: 62 },
    manual: [
      { id: 1, lookId: "red-alert", start: 22 * 3600 + 40 * 60 + 30, end: 22 * 3600 + 42 * 60 + 40, mode: "held", released: 22 * 3600 + 42 * 60 + 40 },
      { id: 2, lookId: "strobe", start: 22 * 3600 + 43 * 60 + 30, end: null, mode: "held", released: null },
    ],
    xyHeld: null,
    rules: { brightness: "latest", manualEnds: "release", lookEnds: "fallback" },
    songSections: Object.fromEntries(
      [...SONGS, ...LIBRARY_ONLY].map((song) => [song.id, song.sections.map((x, i) => ({ ...x, id: `${song.id}-${i}` }))]),
    ),
    // one pass recorded earlier today on Glass Hours
    recorded: {
      "glass-hours": {
        muted: false,
        writing: false,
        passes: [
          {
            id: "pass-1",
            started: 21 * 3600 + 58 * 60,
            events: [
              { at: 556, kind: "start", lookId: "strobe" },
              { at: 586, kind: "stop", lookId: "strobe" },
              { at: 790, kind: "fader", fader: "Wash", value: 40 },
              { at: 965, kind: "start", lookId: "fan-out" },
              { at: 1020, kind: "stop", lookId: "fan-out" },
              { at: 1140, kind: "flash", lookId: "blinder", dur: 2 },
            ],
          },
        ],
      },
    },
    stopped: {},
    lookGroups: [...new Set(Object.values(initialLibrary()).flatMap((i) => i.groups))].sort(),
    jump: {},
    showMuted: {},
    autopilot: {
      on: true,
      style: "club",
      styles: [
        {
          id: "chill",
          name: "Chill",
          rules: [
            { id: "chill-1", when: "intro", pick: "washes", fit: "loop" },
            { id: "chill-2", when: "build", pick: "chases", fit: "loop" },
            { id: "chill-3", when: "drop", pick: "pixels", fit: "loop" },
            { id: "chill-4", when: "break", pick: "washes", fit: "once" },
            { id: "chill-5", when: "outro", pick: "washes", fit: "once" },
          ],
        },
        {
          id: "club",
          name: "Club",
          rules: [
            { id: "club-1", when: "intro", pick: "washes", fit: "loop" },
            { id: "club-2", when: "build", pick: "end of build-up", fit: "ends" },
            { id: "club-3", when: "drop", pick: "drop", fit: "loop" },
            { id: "club-4", when: "drop", pick: "strobes", fit: "once" },
            { id: "club-5", when: "break", pick: "movement", fit: "loop" },
            { id: "club-6", when: "outro", pick: "washes", fit: "once" },
          ],
        },
        {
          id: "peak",
          name: "Peak",
          rules: [
            { id: "peak-1", when: "intro", pick: "chases", fit: "loop" },
            { id: "peak-2", when: "build", pick: "end of build-up", fit: "ends" },
            { id: "peak-3", when: "drop", pick: "strobes", fit: "loop" },
            { id: "peak-4", when: "drop", pick: "pixels", fit: "loop" },
            { id: "peak-5", when: "break", pick: "movement", fit: "loop" },
          ],
        },
        {
          id: "no-strobe",
          name: "No strobe",
          rules: [
            { id: "calm-1", when: "intro", pick: "washes", fit: "loop" },
            { id: "calm-2", when: "build", pick: "chases", fit: "ends" },
            { id: "calm-3", when: "drop", pick: "pixels", fit: "loop" },
            { id: "calm-4", when: "break", pick: "washes", fit: "once" },
          ],
        },
      ],
      wild: 55,
      groups: { movers: true, pars: true, bars: true, laser: false },
      neverDark: "warm-wash",
      swaps: {},
      likes: [],
    },
    events: [],
    history: [],
  });

  let nextId = 100;

  /** Show clock in seconds since midnight. */
  const now = () => CLOCK_START + elapsed() / 1000;
  const beats = () => (elapsed() / 1000) * (state.tempo / 60);
  /** 0..3, the beat in the bar */
  const beat = () => Math.floor(beats()) % 4;
  /** 0..1 within the current beat */
  const beatPhase = () => beats() % 1;

  // ---- undo ----------------------------------------------------------
  /**
   * Put an action on the undo stack. Calls with the same key within a moment
   * of each other merge into one step (a fader drag is one step).
   */
  function record(label: string, undo: () => void, key?: string) {
    const top = state.history[state.history.length - 1];
    const at = performance.now();
    if (key && top && top.key === key && at - top.at < COALESCE_MS) {
      set("history", state.history.length - 1, "at", at);
      return;
    }
    set("history", (h) => [...h.slice(-99), { label, undo, key, at }]);
  }

  function undo() {
    const top = state.history[state.history.length - 1];
    if (!top) return;
    set("history", (h) => h.slice(0, -1));
    top.undo();
  }

  const undoLabel = () => state.history[state.history.length - 1]?.label ?? null;

  // ---- the set -------------------------------------------------------
  /** A song's sections as they are now (they can be moved, split, merged, renamed). */
  const sectionsOf = (song: Song): SongSection[] =>
    state.songSections[song.id] ?? song.sections.map((x, i) => ({ ...x, id: `${song.id}-${i}` }));
  const songLength = (s: Song) => sectionsOf(s).reduce((a, x) => a + x.length, 0);
  const setLengthOf = () => SONGS.reduce((a, s) => a + songLength(s), 0);

  /** Every section placed on the clock that overlaps [from, to]. The set repeats. */
  function sectionsBetween(from: number, to: number): PlacedSection[] {
    const out: PlacedSection[] = [];
    const setLength = setLengthOf();
    const firstCycle = Math.max(0, Math.floor((from - SET_START) / setLength));
    for (let cycle = firstCycle; ; cycle++) {
      let t = SET_START + cycle * setLength;
      if (t > to) break;
      SONGS.forEach((song, songIndex) => {
        const songStart = t;
        sectionsOf(song).forEach((section, index) => {
          const start = t;
          const end = t + section.length;
          t = end;
          if (end > from && start < to) {
            out.push({ key: `${cycle}:${song.id}:${index}`, song, songIndex, section, index, start, end, songStart });
          }
        });
      });
    }
    return out;
  }

  const sectionAt = (t: number) => sectionsBetween(t, t + 0.001)[0] ?? null;
  const currentSection = () => sectionAt(now());
  const nextSection = () => {
    const cur = currentSection();
    return cur ? sectionAt(cur.end + 0.001) : null;
  };
  const currentSong = () => {
    const cur = currentSection();
    if (!cur) return null;
    return { song: cur.song, index: cur.songIndex, start: cur.songStart, end: cur.songStart + songLength(cur.song) };
  };
  /** Sections of the song that is playing, on the clock. */
  const currentSongSections = () => {
    const s = currentSong();
    return s ? sectionsBetween(s.start, s.end - 0.001) : [];
  };

  // ---- the show of a song ---------------------------------------------
  /** Song position in seconds, for the song that plays. */
  const songPos = () => {
    const c = currentSong();
    return c ? now() - c.start : 0;
  };
  /** Blocks from recorded events: start to stop, or a flash. open: still being written, a held look runs up to now. */
  function recBlocks(events: RecEvent[], songLen: number, open = false, pos = songPos()) {
    const out: { lookId: string; start: number; end: number }[] = [];
    const on: Record<string, number> = {};
    for (const ev of [...events].sort((x, y) => x.at - y.at)) {
      if (ev.kind === "start" && ev.lookId) on[ev.lookId] = ev.at;
      if (ev.kind === "stop" && ev.lookId && on[ev.lookId] !== undefined) {
        out.push({ lookId: ev.lookId, start: on[ev.lookId], end: ev.at });
        delete on[ev.lookId];
      }
      if (ev.kind === "flash" && ev.lookId) out.push({ lookId: ev.lookId, start: ev.at, end: ev.at + (ev.dur ?? 1) });
    }
    for (const [id, at] of Object.entries(on)) out.push({ lookId: id, start: at, end: open ? Math.max(at + 1, pos) : songLen });
    return out;
  }
  const NO_RECORDED: Recorded = { muted: false, writing: false, passes: [] };
  const recordedOf = (song: Song): Recorded => state.recorded[song.id] ?? NO_RECORDED;
  /** Passes that play back: all of them, except the one being written. */
  const finishedPasses = (r: Recorded) => (r.writing ? r.passes.slice(0, -1) : r.passes);
  /** What the show plays now: the section's looks and what was recorded, minus what you stopped. */
  function showLooksNow(): string[] {
    const cur = currentSection();
    if (!cur) return [...state.replay];
    const pos = now() - cur.songStart;
    const base = state.showMuted[cur.song.id] ? [] : sectionLooksOf(cur.song, cur.index);
    const r = recordedOf(cur.song);
    const rec = r.muted
      ? []
      : finishedPasses(r).flatMap((x) => recBlocks(x.events, songLength(cur.song)).filter((b) => b.start <= pos && pos < b.end).map((b) => b.lookId));
    const off = state.stopped[cur.key] ?? [];
    return [...new Set([...state.replay, ...rec, ...base])].filter((id) => !off.includes(id));
  }
  /** A section the show already covers: the autopilot leaves it alone. */
  const covered = (p: PlacedSection) => !state.showMuted[p.song.id] && sectionLooksOf(p.song, p.index).length > 0;

  /** One undo step for any change to a song's show. */
  function editSong(song: Song, label: string, fn: () => void) {
    const before = {
      sections: JSON.parse(JSON.stringify(sectionsOf(song))) as SongSection[],
      recorded: JSON.parse(JSON.stringify(recordedOf(song))) as Recorded,
      looks: { ...state.sectionLooks },
      muted: !!state.showMuted[song.id],
    };
    fn();
    record(label, () => {
      set("songSections", song.id, before.sections);
      set("recorded", song.id, before.recorded);
      set("sectionLooks", reconcile(before.looks));
      set("showMuted", song.id, before.muted);
    });
  }

  /** Move the edge between section i and i + 1 by some seconds. */
  function moveSectionEdge(song: Song, i: number, delta: number) {
    const secs = sectionsOf(song);
    if (i < 0 || i >= secs.length - 1) return;
    const d = Math.max(-(secs[i].length - 8), Math.min(secs[i + 1].length - 8, Math.round(delta)));
    if (!d) return;
    const before = JSON.parse(JSON.stringify(secs)) as SongSection[];
    set("songSections", song.id, i, "length", secs[i].length + d);
    set("songSections", song.id, i + 1, "length", secs[i + 1].length - d);
    record(`Move ${secs[i].title} edge`, () => set("songSections", song.id, before), `edge:${song.id}:${i}`);
  }
  let secSeq = 1;
  function splitSection(song: Song, i: number) {
    editSong(song, `Split ${sectionsOf(song)[i].title}`, () => {
      const secs = [...sectionsOf(song)];
      const a = secs[i];
      const half = Math.round(a.length / 2);
      const b2: SongSection = { ...a, id: `${a.id}-s${secSeq++}`, length: a.length - half, title: `${a.title} 2` };
      secs.splice(i, 1, { ...a, length: half }, b2);
      set("songSections", song.id, secs);
      set("sectionLooks", b2.id, [...(state.sectionLooks[a.id] ?? [])]);
    });
  }
  function mergeSections(song: Song, i: number) {
    const secs = sectionsOf(song);
    if (i >= secs.length - 1) return;
    editSong(song, `Merge ${secs[i].title} and ${secs[i + 1].title}`, () => {
      const a = secs[i];
      const b2 = secs[i + 1];
      const merged: SongSection = {
        ...a,
        length: a.length + b2.length,
        energy: (a.energy * a.length + b2.energy * b2.length) / (a.length + b2.length),
      };
      const next = [...secs];
      next.splice(i, 2, merged);
      set("songSections", song.id, next);
      set("sectionLooks", a.id, [...new Set([...(state.sectionLooks[a.id] ?? []), ...(state.sectionLooks[b2.id] ?? [])])]);
    });
  }
  function renameSection(song: Song, i: number, title: string) {
    const t = title.trim();
    if (!t) return;
    const before = sectionsOf(song)[i].title;
    set("songSections", song.id, i, "title", t);
    record(`Rename ${before}`, () => set("songSections", song.id, i, "title", before), `rename:${song.id}:${i}`);
  }

  const setShowMuted = (song: Song, muted: boolean) => editSong(song, muted ? "Mute show" : "Unmute show", () => set("showMuted", song.id, muted));
  const setRecordedMuted = (song: Song, muted: boolean) =>
    editSong(song, muted ? "Mute recorded" : "Unmute recorded", () => set("recorded", song.id, { ...recordedOf(song), muted }));
  /** Clear: everything recorded for this song goes. */
  const clearRecorded = (song: Song) => editSong(song, "Clear recorded", () => set("recorded", song.id, { ...recordedOf(song), writing: false, passes: [] }));
  /** Undo last pass: the newest finished pass goes. */
  function undoLastPass(song: Song) {
    const r = recordedOf(song);
    const done = finishedPasses(r);
    if (!done.length) return;
    const gone = done[done.length - 1].id;
    editSong(song, "Undo last pass", () => set("recorded", song.id, { ...r, passes: r.passes.filter((x) => x.id !== gone) }));
  }

  // recording writes into the Recorded row of the song that plays: a new pass each time
  let recSeq = 2;
  function recordEvent(ev: Omit<RecEvent, "at"> & { at?: number }) {
    if (!isRecording()) return;
    const cur = currentSong();
    if (!cur) return;
    const id = cur.song.id;
    const r = recordedOf(cur.song);
    if (!r.writing) set("recorded", id, { muted: r.muted, writing: true, passes: [...r.passes, { id: `pass-${recSeq++}`, started: now(), events: [] }] });
    const last = state.recorded[id].passes.length - 1;
    set("recorded", id, "passes", last, "events", (es) => [...es, { ...ev, at: ev.at ?? now() - cur.start }]);
  }
  // a pass ends when recording stops or the song changes
  createEffect(() => {
    const rec = isRecording();
    const playing = currentSong()?.song.id;
    for (const [songId, r] of Object.entries(state.recorded)) {
      if (r.writing && (!rec || songId !== playing)) set("recorded", songId, "writing", false);
    }
  });

  const energyWord = (e: number) => (e < 0.4 ? "low" : e < 0.7 ? "rising" : "high");
  /** Why the autopilot picked what it picked, in plain words. */
  const pickReason = (p: PlacedSection) => `${p.section.title}, ${energyWord(p.section.energy)} energy`;


  const sectionKey = (song: Song, index: number) => sectionsOf(song)[index]?.id ?? `${song.id}-${index}`;
  /** The looks a section of the song's show holds. */
  const sectionLooksOf = (song: Song, index: number): string[] => state.sectionLooks[sectionKey(song, index)] ?? [];

  function setSectionLooks(song: Song, index: number, ids: string[], label = "Change section") {
    const key = sectionKey(song, index);
    const before = state.sectionLooks[key];
    set("sectionLooks", key, ids);
    record(label, () =>
      set("sectionLooks", produce((s: Record<string, string[]>) => {
        if (before) s[key] = before;
        else delete s[key];
      })),
    );
  }

  /** Fill empty sections: the autopilot picks once, with the style in use, and the picks are written into the show. */
  function fillEmptySections(song: Song) {
    const before = { ...state.sectionLooks };
    const start = song.id === currentSong()?.song.id ? currentSong()!.start : 0;
    let t = start;
    sectionsOf(song).forEach((sec, i) => {
      const p: PlacedSection = { key: `${song.id}@${start}-${i}`, song, songIndex: 0, section: sec, index: i, start: t, end: t + sec.length, songStart: start };
      t += sec.length;
      if (sectionLooksOf(song, i).length) return;
      const ids = autopilotLooks(p).map((l) => l.id);
      if (ids.length) set("sectionLooks", sectionKey(song, i), ids);
    });
    record("Fill empty sections", () => set("sectionLooks", reconcile(before)));
  }
  const analysisOf = (song: Song): Song["analysis"] => (state.reviewed.includes(song.id) ? "analysed" : song.analysis);
  function markReviewed(song: Song) {
    if (state.reviewed.includes(song.id)) return;
    set("reviewed", (r) => [...r, song.id]);
    record(`Checked ${song.title}`, () => set("reviewed", (r) => r.filter((x) => x !== song.id)));
  }


  const activeStyle = () => state.autopilot.styles.find((x) => x.id === state.autopilot.style) ?? state.autopilot.styles[0];
  const hash = (text: string) => [...text].reduce((a, c) => (a * 31 + c.charCodeAt(0)) >>> 0, 7);
  /** Looks of a library group the autopilot may use: their lights must be allowed. */
  const pickable = (group: string) =>
    Object.values(state.library).filter((i) => i.groups.includes(group) && fixturesOf(i.id).some((g) => state.autopilot.groups[g]));
  /** What the autopilot plays in a section: one look per rule of the style in use, each picked from the rule's group. */
  function autopilotLooks(p: PlacedSection): Look[] {
    const off = state.stopped[p.key] ?? [];
    const swapped = state.autopilot.swaps[p.key];
    if (swapped) return swapped.filter((id) => !off.includes(id)).map(lookById);
    const picks = activeStyle()
      .rules.filter((r) => r.when === p.section.label)
      .map((r) => {
        const from = pickable(r.pick);
        return from.length ? from[hash(p.key + r.id) % from.length].id : null;
      })
      .filter((id): id is string => !!id && !off.includes(id));
    return [...new Set(picks)].map(lookById);
  }

  /** The autopilot's looks now. With nothing to pick and nobody else playing, "Never dark" plays. */
  const autopilotNow = (): Look[] => {
    const cur = currentSection();
    if (!state.autopilot.on || !autopilotArmed()) return [];
    const picks = cur && !covered(cur) ? autopilotLooks(cur) : [];
    const nd = state.autopilot.neverDark;
    const off = cur ? (state.stopped[cur.key] ?? []) : [];
    return !picks.length && nd && !off.includes(nd) && !showLooksNow().length && !heldManual().length ? [lookById(nd)] : picks;
  };

  // ---- autopilot settings (they live on the Looks screen) -----------------
  let apSeq = 1;
  function editAutopilot(label: string, fn: () => void, key?: string) {
    const before = JSON.parse(JSON.stringify({ styles: state.autopilot.styles, style: state.autopilot.style, neverDark: state.autopilot.neverDark }));
    fn();
    record(label, () => (set("autopilot", "styles", before.styles), set("autopilot", "style", before.style), set("autopilot", "neverDark", before.neverDark)), key);
  }
  const styleIndex = (styleId: string) => state.autopilot.styles.findIndex((x) => x.id === styleId);
  function addStyle() {
    const id = `style-${apSeq++}`;
    editAutopilot("New style", () => set("autopilot", "styles", (ss) => [...ss, { id, name: `Style ${ss.length + 1}`, rules: [] }]));
    return id;
  }
  const renameStyle = (styleId: string, name: string) =>
    editAutopilot("Rename style", () => set("autopilot", "styles", styleIndex(styleId), "name", name), `style-name:${styleId}`);
  function deleteStyle(styleId: string) {
    if (state.autopilot.styles.length < 2) return;
    editAutopilot("Delete style", () => {
      set("autopilot", "styles", (ss) => ss.filter((x) => x.id !== styleId));
      if (state.autopilot.style === styleId) set("autopilot", "style", state.autopilot.styles[0].id);
    });
  }
  function addRule(styleId: string) {
    const id = `rule-${apSeq++}`;
    editAutopilot("New rule", () =>
      set("autopilot", "styles", styleIndex(styleId), "rules", (rs) => [...rs, { id, when: "drop" as const, pick: state.lookGroups[0] ?? "", fit: "loop" as const }]),
    );
    return id;
  }
  const setRule2 = (styleId: string, ruleId: string, patch: Partial<ApRule>) =>
    editAutopilot("Change rule", () => set("autopilot", "styles", styleIndex(styleId), "rules", (r) => r.id === ruleId, patch));
  const removeRule = (styleId: string, ruleId: string) =>
    editAutopilot("Remove rule", () => set("autopilot", "styles", styleIndex(styleId), "rules", (rs) => rs.filter((r) => r.id !== ruleId)));
  const setNeverDark = (lookId: string | null) => editAutopilot("Never dark", () => set("autopilot", "neverDark", lookId));

  /** Skip a whole pick: every look in it is replaced with another of the same kind. */
  function skipPick(p: PlacedSection) {
    const before = state.autopilot.swaps[p.key];
    const ids = autopilotLooks(p).map((l) => l.id);
    const next = ids.map((id) => {
      const old = lookById(id);
      const options = LOOKS.filter(
        (l) => l.kind === old.kind && !ids.includes(l.id) && l.id !== "blackout-hold" && l.groups.some((g) => old.groups.includes(g)),
      );
      return (options[Math.floor(Math.random() * options.length)] ?? old).id;
    });
    set("autopilot", "swaps", p.key, next);
    record(`Skip ${p.section.title}`, () =>
      set("autopilot", "swaps", produce((s: Record<string, string[]>) => {
        if (before) s[p.key] = before;
        else delete s[p.key];
      })),
    );
  }

  function morePick(p: PlacedSection) {
    const before = [...state.autopilot.likes];
    const add = autopilotLooks(p).map((l) => l.id).filter((id) => !before.includes(id));
    if (!add.length) return;
    set("autopilot", "likes", [...before, ...add]);
    record(`More like ${p.section.title}`, () => set("autopilot", "likes", before));
  }

  /** Keep a pick: it becomes normal blocks in the song's show that you own. */
  function keepPick(p: PlacedSection) {
    const looks = autopilotLooks(p);
    if (!looks.length) return;
    setSectionLooks(p.song, p.index, looks.map((l) => l.id), `Keep ${p.section.title}`);
  }

  // ---- decks and the set ---------------------------------------------
  /** The song that plays sits on deck 1 when its place in the set is even (song 2 is on deck 1). */
  function decks(): Deck[] {
    const cur = currentSong();
    if (!cur) return [];
    const next = SONGS[(cur.index + 1) % SONGS.length];
    const playingDeck: Deck["number"] = cur.index % 2 === 1 ? "1" : "2";
    const other: Deck["number"] = playingDeck === "1" ? "2" : "1";
    const list: Deck[] = [
      { number: playingDeck, song: cur.song, playing: true, position: now() - cur.start, length: cur.end - cur.start },
      state.cuedDeckLoaded
        ? { number: other, song: next, playing: false, position: 0, length: songLength(next) }
        : { number: other, song: null, playing: false, position: 0, length: 0 },
    ];
    return list.sort((a, b) => a.number.localeCompare(b.number));
  }

  // ---- manual: what you touch stays yours until Release ---------------
  /** Pads that are on. */
  const runningManual = () => state.manual.filter((b) => b.end === null);
  /** Everything a pad took under manual control and that was not released yet, pad on or off. */
  const heldManual = () => state.manual.filter((b) => b.released === null);
  const manualBlockFor = (lookId: string) => runningManual().find((b) => b.lookId === lookId);

  function log(lookId: string, action: ManualEvent["action"]) {
    if (!isRecording()) return;
    set("events", (e) => [...e, { at: now(), lookId, action }]);
    // a flash is written when it ends, with its length
    if (action !== "flash") recordEvent({ kind: action, lookId });
  }

  const removeBlock = (id: number) => set("manual", (bs) => bs.filter((b) => b.id !== id));
  const setBlock = (id: number, patch: Partial<ManualBlock>) => set("manual", (b) => b.id === id, patch);

  /** Start a look by hand and keep it running. A flash in progress becomes held. */
  function start(lookId: string) {
    const running = manualBlockFor(lookId);
    if (running && running.mode === "held") return;
    let id: number;
    // the same pad again: it takes over what its earlier press still held
    const t = now();
    set("manual", (b) => b.lookId === lookId && b.released === null && b.end !== null, "released", t);
    if (running) {
      id = running.id;
      setBlock(id, { mode: "held" });
    } else {
      id = nextId++;
      set("manual", (bs) => [...bs, { id, lookId, start: t, end: null, mode: "held", released: null }]);
    }
    log(lookId, "start");
    record(`Start ${lookById(lookId).name}`, () => removeBlock(id));
  }

  /** Switch a pad off. What it touched stays under manual control until Release, unless the rule says it ends with the pad (or andRelease: a try in the editor). */
  function stop(lookId: string, andRelease = false) {
    const running = manualBlockFor(lookId);
    if (!running) return;
    const id = running.id;
    const t = now();
    setBlock(id, { end: t, released: andRelease || state.rules.manualEnds === "pad" ? t : null });
    log(lookId, "stop");
    record(`Stop ${lookById(lookId).name}`, () => setBlock(id, { end: null, released: null }));
  }

  /** Flash a look while a finger is down. Not an undo step. */
  function hold(lookId: string) {
    if (manualBlockFor(lookId)) return;
    set("manual", (bs) => [...bs, { id: nextId++, lookId, start: now(), end: null, mode: "flash", released: null }]);
    log(lookId, "flash");
  }

  /** End a flash. A flash is only there while the finger is down: it holds nothing afterwards. */
  function release(lookId: string) {
    const running = manualBlockFor(lookId);
    if (!running || running.mode !== "flash") return;
    const t = now();
    setBlock(running.id, { end: t, released: t });
    const cur = currentSong();
    if (cur) recordEvent({ kind: "flash", lookId, at: running.start - cur.start, dur: t - running.start });
  }

  /** Release: manual control ends for everything the Console holds. Pads that are on go off. */
  function releaseAll() {
    const before = heldManual().map((b) => ({ id: b.id, end: b.end }));
    const xy = state.xyHeld;
    if (!before.length && xy === null) return;
    const t = now();
    for (const b of before) setBlock(b.id, { end: b.end ?? t, released: t });
    set("xyHeld", null);
    record("Release", () => {
      for (const b of before) setBlock(b.id, { end: b.end, released: null });
      set("xyHeld", xy);
    });
  }

  function surprise() {
    const idle = LOOKS.filter((l) => !manualBlockFor(l.id) && l.id !== "blackout-hold");
    start(idle[Math.floor(Math.random() * idle.length)].id);
  }

  // ---- master, blackout, output --------------------------------------
  function setMaster(v: number) {
    const before = state.master;
    set("master", clamp(v));
    record("Master", () => set("master", before), "master");
    recordFader("Master", clamp(v));
  }

  function setBlackout(on: boolean) {
    if (state.blackout === on) return;
    set("blackout", on);
    record(on ? "Blackout" : "Blackout off", () => set("blackout", !on));
  }

  /** Edge states to look at: no lights connected, a deck with nothing loaded. */
  const setLightsConnected = (on: boolean) => set("lightsConnected", on);
  const setCuedDeckLoaded = (on: boolean) => set("cuedDeckLoaded", on);

  // ---- patch ----------------------------------------------------------
  const fixtureById = (id: string) => state.rig.find((f) => f.id === id);
  const lastAddress = (f: Fixture) => f.address + f.channels - 1;
  /** Fixtures whose addresses overlap another fixture in the same universe. */
  function conflicts(): { a: Fixture; b: Fixture; from: number; to: number }[] {
    const out: { a: Fixture; b: Fixture; from: number; to: number }[] = [];
    state.rig.forEach((a, i) =>
      state.rig.slice(i + 1).forEach((b) => {
        if (a.universe !== b.universe) return;
        const from = Math.max(a.address, b.address);
        const to = Math.min(lastAddress(a), lastAddress(b));
        if (from <= to) out.push({ a, b, from, to });
      }),
    );
    return out;
  }
  const inConflict = (id: string) => conflicts().some((c) => c.a.id === id || c.b.id === id);

  /** The first address where `channels` fit without touching anything. */
  function freeAddress(channels: number, universe = 1, except?: string) {
    const used = state.rig.filter((f) => f.universe === universe && f.id !== except);
    for (let a = 1; a + channels - 1 <= 512; a++) {
      const b = a + channels - 1;
      if (!used.some((f) => a <= lastAddress(f) && f.address <= b)) return a;
    }
    return null;
  }

  function moveFixture(id: string, x: number, y: number) {
    const f = fixtureById(id);
    if (!f) return;
    const before = { x: f.x, y: f.y };
    set("rig", (r) => r.id === id, { x: Math.max(0.03, Math.min(0.97, x)), y: Math.max(0.08, Math.min(0.9, y)) });
    record(`Move ${f.name}`, () => set("rig", (r) => r.id === id, before), `move:${id}`);
  }

  function setAddress(id: string, address: number) {
    const f = fixtureById(id);
    if (!f) return;
    const before = f.address;
    const a = Math.max(1, Math.min(513 - f.channels, Math.round(address)));
    set("rig", (r) => r.id === id, "address", a);
    record(`Address ${f.name}`, () => set("rig", (r) => r.id === id, "address", before), `addr:${id}`);
  }

  function addFixture(f: Fixture) {
    set("rig", (r) => [...r, f]);
    record(`Add ${f.name}`, () => set("rig", (r) => r.filter((x) => x.id !== f.id)));
  }

  const setIdentify = (id: string | null) => set("identify", id);

  // ---- pixel panels and universes -----------------------------------------
  /** A panel spread over universes: 170 RGB pixels each, or 128 RGBW. */
  function panelRuns(panel: PixelPanel): PanelRun[] {
    const total = panel.cols * panel.rows;
    const per = panel.pixel === "RGBW" ? 4 : 3;
    const PIXELS_PER_UNIVERSE = Math.floor(512 / per);
    const out: PanelRun[] = [];
    for (let first = 1, u = panel.startUniverse; first <= total; first += PIXELS_PER_UNIVERSE, u++) {
      const last = Math.min(total, first + PIXELS_PER_UNIVERSE - 1);
      out.push({ panel, universe: u, address: 1, channels: (last - first + 1) * per, firstPixel: first, lastPixel: last });
    }
    return out;
  }
  const allPanelRuns = () => state.panels.flatMap(panelRuns);
  /** Every universe in use, with what fills it. */
  function universes() {
    const max = Math.max(1, ...state.rig.map((f) => f.universe), ...allPanelRuns().map((r) => r.universe));
    return Array.from({ length: max }, (_, i) => {
      const u = i + 1;
      const fx = state.rig.filter((f) => f.universe === u);
      const runs = allPanelRuns().filter((r) => r.universe === u);
      const used = fx.reduce((a, f) => a + f.channels, 0) + runs.reduce((a, r) => a + r.channels, 0);
      return {
        universe: u,
        fixtures: fx.length,
        used: Math.min(512, used),
        panel: runs[0]?.panel ?? null,
        conflict: conflicts().some((c) => c.a.universe === u),
      };
    });
  }
  const firstFreeUniverse = () => universes().length + 1;
  function addPanel(p: Omit<PixelPanel, "id">) {
    const id = `panel-${Date.now().toString(36)}`;
    set("panels", (ps) => [...ps, { ...p, id }]);
    record(`Add ${p.name}`, () => set("panels", (ps) => ps.filter((x) => x.id !== id)));
    return id;
  }

  /** Move a fixture up or down in its group: the order is what "left to right" means. */
  function reorderFixture(id: string, delta: number) {
    const f = fixtureById(id);
    if (!f) return;
    const same = state.rig.filter((x) => x.group === f.group);
    const i = same.findIndex((x) => x.id === id);
    const j = Math.max(0, Math.min(same.length - 1, i + delta));
    if (i === j) return;
    const before = [...state.rig];
    const order = [...same];
    const [moved] = order.splice(i, 1);
    order.splice(j, 0, moved);
    let k = 0;
    const next = state.rig.map((x) => (x.group === f.group ? order[k++] : x));
    set("rig", next.map((x) => ({ ...x })));
    record(`Order ${f.name}`, () => set("rig", before.map((x) => ({ ...x }))), `order:${id}`);
  }

  // ---- DMX --------------------------------------------------------------
  /** Channel names per fixture, in plain words where they fit. */
  function attributes(f: Fixture): string[] {
    if (f.kind === "par") return ["Dimmer", "Red", "Green", "Blue", "White", "Strobe", "Program"].slice(0, f.channels);
    if (f.kind === "mover")
      return ["Pan", "Pan fine", "Tilt", "Tilt fine", "Speed", "Dimmer", "Shutter", "Red", "Green", "Blue", "White", "Colour", "Gobo", "Gobo spin", "Prism", "Focus"].slice(0, f.channels);
    if (f.kind === "bar") return Array.from({ length: f.channels }, (_, i) => `${["Red", "Green", "Blue"][i % 3]} ${Math.floor(i / 3) + 1}`);
    return ["Mode", "Pattern", "Size", "Rotate", "X", "Y", "Red", "Green", "Blue", "Speed"].slice(0, f.channels);
  }

  const rgb = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));

  /** The 512 values of a universe (index 1..512). phase shifts the beat, to find what changes. */
  function channelValues(universe: number, phase = 0): number[] {
    const v = new Array<number>(513).fill(0);
    const bp = (beatPhase() + phase) % 1;
    for (const f of state.rig) {
      if (f.universe !== universe) continue;
      const { look, level, strobe, position } = fixtureState(f);
      // position: from the pad if you hold it, moving if a look has it, else at rest
      const swing = (offset: number) => Math.round(127 + 100 * Math.sin(((beats() + phase) / 2 + offset) * Math.PI));
      const pan = position?.look.id === "xy" ? Math.round((state.xy.pan / 100) * 255) : position ? swing(0) : 127;
      const tilt = position?.look.id === "xy" ? Math.round((state.xy.tilt / 100) * 255) : position ? swing(0.5) : 127;
      const [r, g, b] = look ? rgb(look.colors[0]) : [0, 0, 0];
      const dim = Math.round(level * 255);
      const names = attributes(f);
      names.forEach((name, i) => {
        let x = 0;
        if (name === "Dimmer") x = dim;
        else if (name === "Red") x = r;
        else if (name === "Green") x = g;
        else if (name === "Blue") x = b;
        else if (name === "White") x = look && r > 200 && g > 200 && b > 200 ? 255 : 0;
        else if (name === "Strobe") x = strobe ? Math.round((state.faders.strobeRate / 100) * 255) : 0;
        else if (name === "Pan") x = pan;
        else if (name === "Tilt") x = tilt;
        else if (name === "Shutter") x = dim ? 255 : 0;
        else if (name === "Speed") x = 40;
        else if (f.kind === "bar") {
          // pixel chases run along the bar with the beat
          const px = Math.floor(i / 3);
          const wave = look?.kind === "pixels" ? Math.max(0, 1 - Math.abs(((px / 8 - bp + 1) % 1) - 0.5) * 3) : 1;
          x = Math.round([r, g, b][i % 3] * level * wave);
        } else if (f.kind === "laser") x = level ? [64, 12, 128, Math.round(bp * 255), 128, 128, r, g, b, 90][i] ?? 0 : 0;
        else x = 0;
        v[f.address + i] = x;
      });
    }
    if (universe === 1) for (const [a, x] of Object.entries(state.held)) v[Number(a)] = x;
    return v;
  }

  function holdChannel(address: number, value: number) {
    const before = state.held[address];
    set("held", address, Math.max(0, Math.min(255, Math.round(value))));
    record(`Hold channel ${address}`, () =>
      set("held", produce((h: Record<number, number>) => {
        if (before === undefined) delete h[address];
        else h[address] = before;
      })),
      `held:${address}`,
    );
  }
  function releaseChannel(address: number) {
    const before = state.held[address];
    if (before === undefined) return;
    set("held", produce((h: Record<number, number>) => void delete h[address]));
    record(`Release channel ${address}`, () => set("held", address, before));
  }
  function releaseAllChannels() {
    const before = { ...state.held };
    if (!Object.keys(before).length) return;
    set("held", reconcile({}));
    record("Release all channels", () => set("held", reconcile(before)));
  }

  // ---- outputs and clock -------------------------------------------------
  /** The output devices. The USB adapter drops out now and then and reconnects. */
  function outputs(): Output[] {
    const sec = elapsed() / 1000;
    const usbDown = sec % 45 < 6;
    const jitter = Math.floor(sec) % 3 === 0 ? 43 : 44;
    return [
      {
        id: "usb",
        universe: 1,
        device: "USB DMX adapter",
        kind: "USB DMX",
        state: usbDown ? "reconnecting" : "connected",
        fps: usbDown ? 0 : jitter,
        errors: 2,
        detail: usbDown ? "Lost the adapter, trying again" : "Sending",
      },
      {
        id: "artnet",
        universe: 2,
        device: "Art-Net node 2.0.0.12",
        kind: "Art-Net",
        state: "error",
        fps: 0,
        errors: 12 + Math.floor(sec / 5),
        detail: "No reply since 22:41",
      },
      { id: "sacn", universe: 5, device: "sACN, bar wall", kind: "sACN", state: "connected", fps: 40, errors: 0, detail: "Sending" },
      { id: "ddp", universe: null, device: "Pixel controller 10.0.0.40", kind: "DDP", state: "connected", fps: 60, errors: 0, detail: "Sending, 480 pixels" },
    ];
  }

  function setClockFeed(feed: ClockFeed) {
    const before = state.clockFeed;
    if (before === feed) return;
    set("clockFeed", feed);
    record("Clock", () => set("clockFeed", before));
  }

  /** Tap tempo: after two taps the tempo follows them and the clock switches to Tap. */
  let taps: number[] = [];
  function tap() {
    const t = performance.now();
    taps = [...taps.filter((x) => t - x < 3000), t].slice(-5);
    if (taps.length < 2) return null;
    const gaps = taps.slice(1).map((x, i) => x - taps[i]);
    const bpm = 60000 / (gaps.reduce((a, b) => a + b, 0) / gaps.length);
    if (state.clockFeed !== "tap") set("clockFeed", "tap");
    setTempo(bpm);
    return bpm;
  }
  const tapCount = () => taps.length;

  function setTempo(bpm: number) {
    const before = state.tempo;
    set("tempo", Math.max(40, Math.min(220, Math.round(bpm * 10) / 10)));
    record("Tempo", () => set("tempo", before), "tempo");
  }

  /** The show replays these looks (no undo: a replay is a performance). */
  const setReplay = (ids: string[]) => set("replay", ids);

  // ---- looks library ---------------------------------------------------
  const item = (id: string): Item | undefined => state.library[id];
  const items = () => Object.values(state.library);

  /** Colours a thing shows: a step's own, a pattern's step, everything in a sequence. */
  function colorsOf(id: string, seen = new Set<string>()): string[] {
    const it = item(id);
    if (!it || seen.has(id)) return [];
    seen.add(id);
    if (it.kind === "step") return it.colour ?? [];
    if (it.kind === "pattern") return it.colours ? colorsOf(it.colours, seen) : it.ownColours;
    return [...new Set(it.entries.flatMap((x) => colorsOf(x.ref, seen)))];
  }
  /** Fixture groups a thing reaches. */
  function fixturesOf(id: string, seen = new Set<string>()): GroupId[] {
    const it = item(id);
    if (!it || seen.has(id)) return [];
    seen.add(id);
    if (it.kind === "step") return it.fixtures;
    if (it.kind === "pattern") return [it.on];
    return [...new Set(it.entries.flatMap((x) => fixturesOf(x.ref, seen)))];
  }
  /** Beats per loop. Steps have none. */
  const measureOf = (id: string): number | null => {
    const it = item(id);
    if (!it || it.kind === "step") return null;
    if (it.kind === "pattern") return it.measure;
    return Math.max(0, ...it.entries.map((x) => x.start + x.width));
  };
  /** Every sequence that uses this thing directly. */
  const usedIn = (id: string): Sequence[] =>
    items().filter((i): i is Sequence => i.kind === "sequence" && i.entries.some((x) => x.ref === id));
  const allGroups = () => state.lookGroups;

  // every library item plays like a look
  setLookResolver((id) => {
    const it = state.library[id];
    if (!it) return undefined;
    const colors = colorsOf(id);
    return { id, name: it.name, colors: colors.length ? colors : ["#000000"], groups: fixturesOf(id), kind: it.lookKind };
  });

  /** Change one item, as one undo step (coalesced per item and field). */
  function editItem(id: string, label: string, fn: (it: Item) => void, key?: string) {
    const before = JSON.parse(JSON.stringify(state.library[id])) as Item;
    set("library", id, produce((it: Item) => fn(it)));
    record(label, () => set("library", id, reconcile(before)), key);
  }

  function setStep<K extends "colour" | "position" | "gobo" | "dimmer" | "spin" | "fixtures">(id: string, prop: K, value: Step[K] | undefined) {
    editItem(id, `${state.library[id]?.name}: ${prop}`, (it) => {
      if (it.kind !== "step") return;
      if (value === undefined) delete it[prop];
      else (it as Step)[prop] = value as Step[K];
    }, `step:${id}:${prop}`);
  }

  function setEntry(seqId: string, entryId: string, patch: Partial<Entry>, label = "Change item") {
    editItem(seqId, label, (it) => {
      if (it.kind !== "sequence") return;
      const x = it.entries.find((en) => en.id === entryId);
      if (x) Object.assign(x, patch);
    }, `entry:${entryId}:${Object.keys(patch).join()}`);
  }

  function setPattern(id: string, patch: Partial<Pattern>) {
    editItem(id, `${state.library[id]?.name}: pattern`, (it) => {
      if (it.kind === "pattern") Object.assign(it, patch);
    }, `pattern:${id}:${Object.keys(patch).join()}`);
  }

  function addToGroup(id: string, group: string) {
    const g = group.trim();
    if (!g || state.library[id]?.groups.includes(g)) return;
    if (!state.lookGroups.includes(g)) set("lookGroups", (gs) => [...gs, g]);
    editItem(id, `Add to ${g}`, (it) => void it.groups.push(g));
  }
  /** New group: empty until looks are dragged in. Returns the name it got. */
  function addGroup(name = "New group") {
    let n = name.trim() || "New group";
    for (let i = 2; state.lookGroups.includes(n); i++) n = `${name} ${i}`;
    set("lookGroups", (gs) => [...gs, n]);
    record(`New group ${n}`, () => set("lookGroups", (gs) => gs.filter((x) => x !== n)));
    return n;
  }
  /** What a group change touches: the list, the looks' labels and the autopilot rules that pick from it. */
  function editGroups(label: string, fn: () => void) {
    const before = JSON.parse(JSON.stringify({ groups: state.lookGroups, library: state.library, styles: state.autopilot.styles }));
    fn();
    record(label, () => (set("lookGroups", before.groups), set("library", reconcile(before.library)), set("autopilot", "styles", before.styles)));
  }
  function renameGroup(from: string, to: string) {
    const name = to.trim();
    if (!name || name === from || state.lookGroups.includes(name)) return;
    editGroups(`Rename ${from}`, () => {
      set("lookGroups", (gs) => gs.map((g) => (g === from ? name : g)));
      for (const it of Object.values(state.library)) if (it.groups.includes(from)) set("library", it.id, "groups", (gs) => gs.map((g) => (g === from ? name : g)));
      state.autopilot.styles.forEach((_, i) => set("autopilot", "styles", i, "rules", (r) => r.pick === from, "pick", name));
    });
  }
  /** Delete a group. The looks stay in the library. */
  function deleteGroup(name: string) {
    editGroups(`Delete ${name}`, () => {
      set("lookGroups", (gs) => gs.filter((g) => g !== name));
      for (const it of Object.values(state.library)) if (it.groups.includes(name)) set("library", it.id, "groups", (gs) => gs.filter((g) => g !== name));
    });
  }

  // ---- connections: what a look is made of and who uses it -----------------
  /** The looks it uses directly. */
  function madeOf(id: string): Item[] {
    const it = state.library[id];
    const refs = !it ? [] : it.kind === "sequence" ? it.entries.map((x) => x.ref) : it.kind === "pattern" && it.colours ? [it.colours] : [];
    return [...new Set(refs)].map((r) => state.library[r]).filter((x) => !!x);
  }
  /** Console pads that play it: page and pad number. */
  const padsOf = (id: string) =>
    Object.entries(state.pads).flatMap(([page, ids]) => ids.map((x, i) => (x === id ? { page, pad: i + 1 } : null)).filter((x) => x !== null));
  /** Song sections whose show holds it. */
  const sectionsUsing = (id: string) =>
    [...SONGS, ...LIBRARY_ONLY].flatMap((song) =>
      sectionsOf(song)
        .map((sec, index) => ((state.sectionLooks[sec.id] ?? []).includes(id) ? { song, index, title: sec.title } : null))
        .filter((x) => x !== null),
    );
  /** Autopilot rules that may pick it: rules whose group it is in. */
  const rulesUsing = (id: string) =>
    state.autopilot.styles.flatMap((style) => style.rules.filter((r) => state.library[id]?.groups.includes(r.pick)).map((rule) => ({ style, rule })));

  /** Jump to a place on another screen. The screen reads state.jump when it opens. */
  function jumpTo(screen: string, jump: Jump) {
    set("jump", reconcile(jump));
    location.hash = `/${screen}`;
  }

  function removeFromGroup(id: string, group: string) {
    editItem(id, `Remove from ${group}`, (it) => {
      it.groups = it.groups.filter((x) => x !== group);
    });
  }

  /** Put a look on the Console: the last pad of the Main page. */
  function putOnConsole(id: string) {
    const before = [...state.pads.main];
    if (before.includes(id)) return before.indexOf(id) + 1;
    set("pads", "main", before.length - 1, id);
    record(`Put ${state.library[id]?.name} on Console`, () => set("pads", "main", before));
    return before.length;
  }

  // ---- learn: mappings ----------------------------------------------------
  let mapSeq = 3;
  function addMapping(m: Omit<Mapping, "id">) {
    const id = `m${mapSeq++}`;
    set("mappings", (ms) => [...ms.filter((x) => !(x.device === m.device && x.message === m.message)), { ...m, id }]);
    record(`Learn ${m.control}`, () => set("mappings", (ms) => ms.filter((x) => x.id !== id)));
    return id;
  }
  function updateMapping(id: string, patch: Partial<Mapping>) {
    const before = state.mappings.find((m) => m.id === id);
    if (!before) return;
    const copy = { ...before };
    set("mappings", (m) => m.id === id, patch);
    record(`Change mapping ${before.control}`, () => set("mappings", (m) => m.id === id, copy), `map:${id}`);
  }
  function removeMapping(id: string) {
    const before = [...state.mappings];
    set("mappings", (ms) => ms.filter((m) => m.id !== id));
    record("Remove mapping", () => set("mappings", before));
  }

  /** Space bar: blackout only while held. */
  const setBlackoutHold = (on: boolean) => set("blackoutHold", on);
  const isDark = () => state.blackout || state.blackoutHold;

  /** LIVE on: performing. LIVE off: preparing. */
  function setLive(on: boolean) {
    if (state.night.live === on) return;
    set("night", "live", on);
    record(on ? "LIVE on" : "LIVE off", () => set("night", "live", !on));
  }

  function toggleIndicator(i: Indicator) {
    const before = state.night.indicators[i];
    set("night", "indicators", i, !before);
    record(`${INDICATOR_NAME[i]} ${before ? "off" : "on"}`, () => set("night", "indicators", i, before));
  }

  /** An indicator is in effect when LIVE is on and it is not turned off. */
  const inEffect = (i: Indicator) => state.night.live && state.night.indicators[i];
  const isRecording = () => inEffect("record");
  const surfaceLocked = () => inEffect("lock");
  const followsDj = () => inEffect("dj");
  const autopilotArmed = () => inEffect("autopilot");

  const FADER_NAME: Record<FaderId, string> = { wash: "Wash", movers: "Movers", pixels: "Pixels", strobeRate: "Strobe rate", laser: "Laser" };
  /** A fader move is a command too: a recording keeps the last value of each drag. */
  function recordFader(fader: string, value: number) {
    const cur = currentSong();
    if (!isRecording() || !cur) return;
    const r = recordedOf(cur.song);
    const pass = r.writing ? r.passes[r.passes.length - 1] : undefined;
    const last = pass?.events[pass.events.length - 1];
    const at = now() - cur.start;
    if (pass && last?.kind === "fader" && last.fader === fader && at - last.at < 1.5) {
      set("recorded", cur.song.id, "passes", r.passes.length - 1, "events", pass.events.length - 1, { at, value });
    } else recordEvent({ kind: "fader", fader, value });
  }

  function setFader(id: FaderId, v: number) {
    const before = state.faders[id];
    set("faders", id, clamp(v));
    record("Fader", () => set("faders", id, before), `fader:${id}`);
    recordFader(FADER_NAME[id], clamp(v));
  }

  function setXY(pan: number, tilt: number) {
    const before = { ...state.xy };
    const held = state.xyHeld;
    set("xy", { pan: clamp(pan), tilt: clamp(tilt) });
    if (held === null) set("xyHeld", now());
    // if manual control ends with the control, the position goes back shortly after the finger stops
    clearTimeout(xyTimer);
    if (state.rules.manualEnds === "pad") xyTimer = window.setTimeout(() => set("xyHeld", null), 900);
    record("Movers position", () => (set("xy", before), set("xyHeld", held)), "xy");
  }
  let xyTimer: number | undefined;

  // ---- autopilot -----------------------------------------------------
  function setAutopilot<K extends "on" | "style" | "wild">(key: K, value: State["autopilot"][K]) {
    const before = state.autopilot[key];
    set("autopilot", key, value as never);
    record(`Autopilot ${key}`, () => set("autopilot", key, before as never), key === "wild" ? "ap:wild" : undefined);
  }

  function toggleGroup(g: GroupId) {
    const before = state.autopilot.groups[g];
    set("autopilot", "groups", g, !before);
    record("Autopilot groups", () => set("autopilot", "groups", g, before));
  }

  /** Replace one look in the current section with another of the same kind. */
  function skipLook(lookId: string) {
    const cur = currentSection();
    if (!cur) return;
    const before = state.autopilot.swaps[cur.key];
    const ids = before ?? cur.section.looks;
    const old = lookById(lookId);
    const options = LOOKS.filter(
      (l) => l.kind === old.kind && !ids.includes(l.id) && l.id !== "blackout-hold" && l.groups.some((g) => old.groups.includes(g)),
    );
    const pick = options[Math.floor(Math.random() * options.length)] ?? LOOKS.find((l) => !ids.includes(l.id))!;
    set("autopilot", "swaps", cur.key, ids.map((id) => (id === lookId ? pick.id : id)));
    record(`Skip ${old.name}`, () =>
      set("autopilot", "swaps", produce((s: Record<string, string[]>) => {
        if (before) s[cur.key] = before;
        else delete s[cur.key];
      })),
    );
  }

  function moreLikeThis(lookId: string) {
    if (state.autopilot.likes.includes(lookId)) return;
    set("autopilot", "likes", (l) => [...l, lookId]);
    record(`More like ${lookById(lookId).name}`, () =>
      set("autopilot", "likes", (l) => l.filter((x) => x !== lookId)),
    );
  }

  // ---- who wins: the one place that decides between two values ---------
  /** What a look sets. A step from the library says it itself; older looks go by their kind. */
  function propsOf(look: Look): Prop[] {
    const it = state.library[look.id];
    if (it?.kind === "step" && !LOOKS.some((l) => l.id === look.id)) {
      const own: Prop[] = [];
      if (it.colour) own.push("colour");
      if (it.dimmer !== undefined) own.push("brightness");
      if (it.position) own.push("position");
      if (own.length) return own;
    }
    if (look.kind === "movement") return ["position"];
    if (look.kind === "strobe") return ["brightness"];
    return ["colour", "brightness"];
  }
  const BRIGHT: Record<string, number> = { uv: 0.4, "deep-blue": 0.55, breathe: 0.5, "slow-fade": 0.45, pulse: 0.7, "cold-wash": 0.75, "red-alert": 0.9, "blackout-hold": 0 };
  /** How bright a look is on its own, 0..1. */
  const brightnessOf = (look: Look) => BRIGHT[look.id] ?? (look.kind === "strobe" ? 1 : look.kind === "wash" ? 0.8 : 0.7);

  interface Want {
    rank: Layer;
    look: Look;
    started: number;
    padOff?: boolean;
  }
  const XY_LOOK: Look = { id: "xy", name: "Movers position", colors: ["#f4efe6"], groups: ["movers"], kind: "movement" };
  /** Everything that wants something right now, by rank. */
  function wants(): Want[] {
    const cur = currentSection();
    const out: Want[] = heldManual().map((b) => ({ rank: "manual" as const, look: lookById(b.lookId), started: b.start, padOff: b.end !== null }));
    if (state.xyHeld !== null) out.push({ rank: "manual", look: XY_LOOK, started: state.xyHeld });
    showLooksNow().forEach((id, i) => out.push({ rank: "show", look: lookById(id), started: (cur?.start ?? 0) - i }));
    autopilotNow().forEach((l, i) => out.push({ rank: "autopilot", look: l, started: (cur?.start ?? 0) - i }));
    return out;
  }
  /** Rule 5, "stay": what the show or the autopilot last had, per group and property. */
  const leftBehind = new Map<string, { rank: Layer; lookId: string }>();

  /**
   * The rules, in order. 1: each property of each group on its own. 2: manual beats show beats autopilot.
   * 3: among equals the one that started last wins (brightness can be set to "highest").
   * 4 and 5 decide who is still in the list: see stop(), releaseAll() and leftBehind.
   * Returns the winner first, then who is under it.
   */
  function contenders(group: GroupId, prop: Prop): Winner[] {
    const all = wants().filter((w) => w.look.groups.includes(group) && (w.look.id === "xy" ? prop === "position" : propsOf(w.look).includes(prop)));
    const out: Winner[] = [];
    for (const rank of ["manual", "show", "autopilot"] as const) {
      const equals = all.filter((w) => w.rank === rank);
      if (!equals.length) continue;
      const highest = prop === "brightness" && state.rules.brightness === "highest";
      equals.sort((a, b) => (highest ? brightnessOf(b.look) - brightnessOf(a.look) : 0) || b.started - a.started);
      out.push({ rank, look: equals[0].look, padOff: equals[0].padOff });
    }
    const key = `${group}|${prop}`;
    const below = out.find((w) => w.rank !== "manual");
    if (below) leftBehind.set(key, { rank: below.rank, lookId: below.look.id });
    else if (state.rules.lookEnds === "stay" && leftBehind.has(key)) {
      const l = leftBehind.get(key)!;
      out.push({ rank: l.rank, look: lookById(l.lookId), left: true });
    }
    return out;
  }
  const winner = (group: GroupId, prop: Prop): Winner | null => contenders(group, prop)[0] ?? null;

  function setRule<K extends keyof Rules>(key: K, value: Rules[K]) {
    const before = state.rules[key];
    if (before === value) return;
    set("rules", key, value);
    // manual control now ends with the pad: pads that are off hold nothing any more
    if (key === "manualEnds" && value === "pad") {
      const t = now();
      set("manual", (b) => b.released === null && b.end !== null, "released", t);
      set("xyHeld", null);
    }
    if (key === "lookEnds" && value === "fallback") leftBehind.clear();
    record("Who wins", () => set("rules", key, before));
  }

  /** What the Console holds until Release: each pad or control with what it touched. */
  function holds(): Hold[] {
    const pads = heldManual().map((b): Hold => {
      const look = lookById(b.lookId);
      const props = propsOf(look);
      // rule 3 on the Console: another pad of yours has this property instead
      const beaten = props.flatMap((p) => {
        const others = [...new Set(look.groups.map((g) => winner(g, p)?.look).filter((w) => w && w.id !== look.id).map((w) => w!.name))];
        return others.length ? [`${props.length > 1 ? `${p}: ` : ""}${others.join(", ")} ${others.length > 1 ? "have" : "has"} it`] : [];
      });
      return { id: `pad-${b.id}`, name: look.name, since: b.start, live: b.end === null, props, groups: look.groups, beaten };
    });
    return state.xyHeld === null ? pads : [...pads, { id: "xy", name: "Movers position", since: state.xyHeld, live: true, props: ["position"], groups: ["movers"], beaten: [] }];
  }
  /** Every look that runs right now, by rank: the Console's Running panel lists them. */
  function running(): Running[] {
    const out: Running[] = heldManual().map((b) => {
      const l = lookById(b.lookId);
      return { id: `pad-${b.id}`, rank: "manual" as const, lookId: l.id, name: l.name, colors: l.colors, padOff: b.end !== null };
    });
    if (state.xyHeld !== null) out.push({ id: "xy", rank: "manual", lookId: "xy", name: "Movers position", colors: ["#f4efe6"] });
    for (const id of showLooksNow()) out.push({ id: `show-${id}`, rank: "show", lookId: id, name: lookById(id).name, colors: lookById(id).colors });
    for (const l of autopilotNow()) out.push({ id: `autopilot-${l.id}`, rank: "autopilot", lookId: l.id, name: l.name, colors: l.colors });
    return out;
  }
  /** Release one thing you hold. */
  function releaseHold(id: string) {
    if (id === "xy") {
      const xy = state.xyHeld;
      set("xyHeld", null);
      record("Release Movers position", () => set("xyHeld", xy));
      return;
    }
    const b = state.manual.find((x) => `pad-${x.id}` === id);
    if (!b || b.released !== null) return;
    const before = { end: b.end };
    const t = now();
    setBlock(b.id, { end: b.end ?? t, released: t });
    record(`Release ${lookById(b.lookId).name}`, () => setBlock(b.id, { end: before.end, released: null }));
  }
  /** Stop one look of the show or the autopilot for the rest of this section. */
  function stopLook(lookId: string) {
    const cur = currentSection();
    if (!cur) return;
    const before = state.stopped[cur.key];
    set("stopped", cur.key, [...(before ?? []), lookId]);
    record(`Stop ${lookById(lookId).name}`, () => set("stopped", cur.key, before as string[]));
  }

  /** "colour and brightness of PAR 1-8, Bars" */
  const holdText = (h: Hold) => `${h.props.join(" and ")} of ${h.groups.map((g) => GROUPS.find((x) => x.id === g)!.name).join(", ")}`;
  /** Rule 5, "stay": looks of a rank that have ended and still show. */
  const staying = (rank: Layer) => [
    ...new Set(GROUPS.flatMap((g) => PROPS.map((p) => winner(g.id, p))).filter((w) => w?.left && w.rank === rank).map((w) => w!.look.name)),
  ];
  /** Groups where Manual has at least one property. */
  const heldGroups = () => GROUPS.filter((g) => PROPS.some((p) => winner(g.id, p)?.rank === "manual"));

  // ---- derived: what each fixture does --------------------------------
  /** Colour from the colour winner, level from the brightness winner. */
  function fixtureState(f: Fixture): { look: Look | null; layer: Layer | null; level: number; strobe: boolean; position: Winner | null } {
    const colour = winner(f.group, "colour");
    const bright = winner(f.group, "brightness");
    const position = winner(f.group, "position");
    const top = colour ?? bright ?? position;
    const look = top?.look ?? null;
    const groupFader: Record<GroupId, number> = {
      movers: state.faders.movers,
      pars: state.faders.wash,
      bars: state.faders.pixels,
      laser: state.faders.laser,
    };
    if (state.identify === f.id) {
      return { look: { id: "identify", name: "Identify", colors: ["#ffffff"], groups: [f.group], kind: "strobe" }, layer: null, level: beatPhase() < 0.5 ? 1 : 0.15, strobe: false, position };
    }
    const own = bright ? brightnessOf(bright.look) : look ? 0.7 : 0;
    const level = isDark() || !look ? 0 : own * (state.master / 100) * (groupFader[f.group] / 100);
    // a strobe whose pad is off no longer flashes: it stays as it was left
    const strobe = !!bright && bright.look.kind === "strobe" && !bright.padOff && !bright.left;
    return { look, layer: top?.rank ?? null, level, strobe, position };
  }

  /** How a look is running: "held" or "flash" by hand, "kept" (pad off, still held until Release), "show", "autopilot", or null. */
  function lookState(lookId: string): "held" | "flash" | "kept" | "show" | "autopilot" | null {
    const b = manualBlockFor(lookId);
    if (b) return b.mode;
    if (heldManual().some((x) => x.lookId === lookId)) return "kept";
    if (showLooksNow().includes(lookId)) return "show";
    if (autopilotNow().some((l) => l.id === lookId)) return "autopilot";
    return null;
  }

  // ---- lanes for the timetable and the Now strip ---------------------
  function lanes(from: number, to: number): Lane[] {
    const t = now();
    // a block runs until manual control ends: with the pad off it goes on as "held"
    const manualBlocks = state.manual
      .filter((b) => (b.released ?? t) > from && b.start < to)
      .map((b) => ({
        id: `manual-${b.id}`,
        title: lookById(b.lookId).name,
        sub: hhmm(b.start),
        start: b.start,
        end: b.released,
        tail: b.released !== null ? undefined : b.mode === "flash" ? "flash" : b.end === null ? "holding" : "held, pad off",
      }));
    const handGroups = heldGroups().map((g) => g.name);
    const cur = currentSection();
    const placed = sectionsBetween(from, to);
    const showBlocks = placed
      .filter((p) => covered(p))
      .map((p) => ({ id: `show-${p.key}`, title: p.section.title, sub: hhmm(p.start), start: p.start, end: p.end }));
    const showNowLooks = showLooksNow();
    const showNext = sectionsBetween(t, t + 3600).find((p) => p.start > t && covered(p));
    const apBlocks = placed.filter((p) => !covered(p)).map((p) => ({
      id: `ap-${p.key}`,
      title: p.section.title,
      sub: p.key === cur?.key ? `${hhmm(p.start)}\n${mmss(p.end - t)} left` : hhmm(p.start),
      start: p.start,
      end: p.end,
    }));
    const apPlaying = state.autopilot.on && autopilotArmed() && !!cur && !covered(cur);
    return [
      {
        layer: "manual",
        title: "Manual",
        on: handGroups.length > 0,
        chip: handGroups.length ? "holding" : "idle",
        status: handGroups.length ? `holds ${handGroups.join(", ")}` : "nothing held",
        blocks: manualBlocks,
      },
      {
        layer: "show",
        title: "Show",
        on: showNowLooks.length > 0,
        chip: showNowLooks.length ? "playing" : "idle",
        status: state.replay.length
          ? `replays ${state.replay.map((id) => lookById(id).name).join(", ")}`
          : showNowLooks.length
            ? showNowLooks.map((id) => lookById(id).name).join(", ")
            : showNext
              ? `next at ${hhmm(showNext.start)}`
              : "nothing planned",
        blocks: showBlocks,
        empty: "No show for this song",
      },
      {
        layer: "autopilot",
        title: "Autopilot",
        on: apPlaying,
        chip: !state.autopilot.on ? "off" : !autopilotArmed() ? "waiting" : apPlaying ? "playing" : "idle",
        status: staying("autopilot").length
          ? `ended, ${staying("autopilot").join(", ")} stays as it was left`
          : !state.autopilot.on
          ? "turned off"
          : !state.night.live
            ? "starts with LIVE"
            : !autopilotArmed()
              ? "Autopilot is off beside LIVE"
              : cur && covered(cur)
                ? "the Show has this part"
                : cur
                  ? cur.section.title
                  : "nothing to play",
        blocks: apBlocks,
      },
    ];
  }

  return {
    state,
    now,
    beat,
    beatPhase,
    // commands
    start,
    stop,
    hold,
    release,
    releaseAll,
    surprise,
    setMaster,
    setBlackout,
    setBlackoutHold,
    setLightsConnected,
    setCuedDeckLoaded,
    setLive,
    toggleIndicator,
    setFader,
    setXY,
    setAutopilot,
    toggleGroup,
    skipLook,
    moreLikeThis,
    skipPick,
    morePick,
    keepPick,
    setRule,
    running,
    releaseHold,
    stopLook,
    addStyle,
    renameStyle,
    deleteStyle,
    addRule,
    setStyleRule: setRule2,
    removeRule,
    setNeverDark,
    addGroup,
    renameGroup,
    deleteGroup,
    madeOf,
    padsOf,
    sectionsUsing,
    rulesUsing,
    jumpTo,
    winner,
    contenders,
    holds,
    holdText,
    heldManual,
    propsOf,
    sectionsOf,
    songLength,
    songPos,
    recBlocks,
    showLooksNow,
    recordedOf,
    setShowMuted,
    setRecordedMuted,
    clearRecorded,
    undoLastPass,
    moveSectionEdge,
    splitSection,
    mergeSections,
    renameSection,
    undo,
    undoLabel,
    record,
    // derived
    isDark,
    inEffect,
    isRecording,
    surfaceLocked,
    followsDj,
    autopilotArmed,
    runningManual,
    lookState,
    fixtureState,
    lanes,
    sectionsBetween,
    currentSection,
    nextSection,
    currentSong,
    currentSongSections,
    autopilotLooks,
    pickReason,
    covered,
    decks,
    autopilotNow,
    get rig() {
      return state.rig;
    },
    sectionLooksOf,
    setSectionLooks,
    fillEmptySections,
    analysisOf,
    markReviewed,
    fixtureById,
    conflicts,
    inConflict,
    freeAddress,
    moveFixture,
    setAddress,
    addFixture,
    setIdentify,
    panelRuns,
    allPanelRuns,
    universes,
    firstFreeUniverse,
    addPanel,
    reorderFixture,
    attributes,
    channelValues,
    holdChannel,
    releaseChannel,
    releaseAllChannels,
    outputs,
    setClockFeed,
    setTempo,
    tap,
    tapCount,
    addMapping,
    updateMapping,
    removeMapping,
    setReplay,
    item,
    items,
    colorsOf,
    fixturesOf,
    measureOf,
    usedIn,
    allGroups,
    setStep,
    setEntry,
    setPattern,
    addToGroup,
    removeFromGroup,
    putOnConsole,
    gobos: GOBOS,
    looks: LOOKS,
    songs: SONGS,
    groups: GROUPS,
  };
}

export const INDICATOR_NAME: Record<Indicator, string> = {
  dj: "DJ link",
  record: "Recording",
  autopilot: "Autopilot",
  lock: "Lock",
};

const clamp = (v: number) => Math.max(0, Math.min(100, Math.round(v)));

/** 22:47 */
export function hhmm(t: number): string {
  const h = Math.floor(t / 3600) % 24;
  const m = Math.floor((t % 3600) / 60);
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`;
}

/** 1:48 */
export function mmss(sec: number): string {
  const s = Math.max(0, Math.floor(sec));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** Group names in plain words: "PAR 1-8, Movers". */
export const groupNames = (ids: GroupId[]) =>
  ids.map((id) => GROUPS.find((g) => g.id === id)!.name).join(", ");

export const store = createRoot(build);
export type Store = typeof store;
