// One fake state store drives every screen. A pad pressed on the Console shows up
// everywhere that reads this store.
import { createRoot, createSignal } from "solid-js";
import { createStore, produce, reconcile } from "solid-js/store";
import {
  CLOCK_START,
  GROUPS,
  LOOKS,
  MASTER,
  RIG,
  SET_START,
  SHOW_NAME,
  SONGS,
  TEMPO,
  lookById,
  type Fixture,
  type GroupId,
  type Layer,
  type Look,
  type Section,
  type Song,
} from "./data";

export * from "./data";

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
export type AutopilotStyle = "chill" | "club" | "peak" | "no-strobe";

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
  letter: "A" | "B";
  /** null: nothing loaded on this deck */
  song: Song | null;
  playing: boolean;
  onAir: boolean;
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
  section: Section;
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

export interface Lane {
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
  /** Looks the Show layer replays right now from a recording (How it works). */
  replay: string[];
  /** false: the cued deck has nothing loaded (an edge state to look at) */
  cuedDeckLoaded: boolean;
  blackout: boolean;
  blackoutHold: boolean;
  faders: Record<FaderId, number>;
  xy: { pan: number; tilt: number };
  manual: ManualBlock[];
  clips: ShowClip[];
  autopilot: {
    on: boolean;
    style: AutopilotStyle;
    wild: number;
    groups: Record<GroupId, boolean>;
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
    sectionLooks: {},
    reviewed: [],
    // three channels held by hand: PAR 1 red, PAR 5 dimmer, Mover R1 pan
    held: { 66: 255, 93: 180, 33: 96 },
    clockFeed: "vdj",
    replay: [],
    cuedDeckLoaded: true,
    blackout: false,
    blackoutHold: false,
    faders: { wash: 70, movers: 60, pixels: 45, strobeRate: 30, laser: 0 },
    xy: { pan: 38, tilt: 62 },
    manual: [
      { id: 1, lookId: "red-alert", start: 22 * 3600 + 40 * 60 + 30, end: 22 * 3600 + 42 * 60 + 40, mode: "held" },
      { id: 2, lookId: "strobe", start: 22 * 3600 + 43 * 60 + 30, end: null, mode: "held" },
    ],
    // Song 1 was recorded. Song 2 has its own show for the first drop only.
    clips: [
      { id: 10, track: "recorded", lookId: "warm-wash", start: 80100, end: 80340, origin: "recorded" },
      { id: 11, track: "recorded", lookId: "tilt-wave", start: 80340, end: 80640, origin: "recorded" },
      { id: 12, track: "recorded", lookId: "pulse", start: 80640, end: 81000, origin: "recorded" },
      { id: 13, track: "recorded", lookId: "deep-blue", start: 81000, end: 81180, origin: "recorded" },
      { id: 14, track: "recorded", lookId: "bass-hit", start: 81180, end: 81420, origin: "recorded" },
      { id: 15, track: "recorded", lookId: "slow-fade", start: 81420, end: 81600, origin: "recorded" },
      { id: 16, track: "looks", lookId: "pulse", start: 82140, end: 82380, origin: "programmed" },
      { id: 17, track: "movers", lookId: "fan-out", start: 82140, end: 82380, origin: "programmed" },
      { id: 18, track: "pixels", lookId: "sparkle", start: 82140, end: 82380, origin: "programmed" },
      { id: 19, track: "laser", lookId: "laser-fan", start: 82200, end: 82380, origin: "programmed" },
    ],
    autopilot: {
      on: true,
      style: "club",
      wild: 55,
      groups: { movers: true, pars: true, bars: true, laser: false },
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
  const songLength = (s: Song) => s.sections.reduce((a, x) => a + x.length, 0);
  const setLength = SONGS.reduce((a, s) => a + songLength(s), 0);

  /** Every section placed on the clock that overlaps [from, to]. The set repeats. */
  function sectionsBetween(from: number, to: number): PlacedSection[] {
    const out: PlacedSection[] = [];
    const firstCycle = Math.max(0, Math.floor((from - SET_START) / setLength));
    for (let cycle = firstCycle; ; cycle++) {
      let t = SET_START + cycle * setLength;
      if (t > to) break;
      SONGS.forEach((song, songIndex) => {
        const songStart = t;
        song.sections.forEach((section, index) => {
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

  // ---- show clips ----------------------------------------------------
  const trackFor = (l: Look): ClipTrack =>
    l.kind === "movement" ? "movers" : l.kind === "pixels" ? "pixels" : l.kind === "laser" ? "laser" : "looks";
  const activeClips = (t = now()) => state.clips.filter((c) => c.start <= t && t < c.end);
  /** A section the show already covers: the autopilot leaves it alone. */
  const covered = (p: PlacedSection) => {
    const mid = (p.start + p.end) / 2;
    return state.clips.some((c) => c.start < mid && c.end > mid);
  };

  function moveClip(id: number, start: number, end: number) {
    const c = state.clips.find((x) => x.id === id);
    if (!c) return;
    const before = { start: c.start, end: c.end };
    set("clips", (x) => x.id === id, { start, end });
    record("Move clip", () => set("clips", (x) => x.id === id, before), `clip:${id}`);
  }

  const energyWord = (e: number) => (e < 0.4 ? "low" : e < 0.7 ? "rising" : "high");
  /** Why the autopilot picked what it picked, in plain words. */
  const pickReason = (p: PlacedSection) => `${p.section.title}, ${energyWord(p.section.energy)} energy`;

  const FALLBACK: Record<string, string[]> = {
    low: ["breathe"],
    rising: ["warm-wash", "tilt-wave"],
    high: ["pulse", "fan-out", "chase-lr"],
  };

  /** What the autopilot plays in a section, after style, groups and skips. */
  // ---- song director ---------------------------------------------------
  const sectionKey = (song: Song, index: number) => `${song.id}:${index}`;
  /** The looks a song section holds: set in the Song Director, else the sample data. */
  const sectionLooksOf = (song: Song, index: number): string[] =>
    state.sectionLooks[sectionKey(song, index)] ?? song.sections[index].looks;

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

  /** Fill every empty section from the autopilot's style. One undo step. */
  function fillFromStyle(song: Song) {
    const before = { ...state.sectionLooks };
    let n = 0;
    song.sections.forEach((sec, i) => {
      if (sectionLooksOf(song, i).length) return;
      const ids = FALLBACK[energyWord(sec.energy)].filter(
        (id) => !(state.autopilot.style === "no-strobe" && lookById(id).kind === "strobe"),
      );
      set("sectionLooks", sectionKey(song, i), ids);
      n++;
    });
    if (n) record(`Fill ${song.title}`, () => set("sectionLooks", reconcile(before)));
    return n;
  }

  const analysisOf = (song: Song): Song["analysis"] => (state.reviewed.includes(song.id) ? "analysed" : song.analysis);
  function markReviewed(song: Song) {
    if (state.reviewed.includes(song.id)) return;
    set("reviewed", (r) => [...r, song.id]);
    record(`Checked ${song.title}`, () => set("reviewed", (r) => r.filter((x) => x !== song.id)));
  }

  /** What the autopilot plays in a section, after style, groups and skips. */
  function autopilotLooks(p: PlacedSection): Look[] {
    const held = sectionLooksOf(p.song, p.index);
    const ids = state.autopilot.swaps[p.key] ?? (held.length ? held : FALLBACK[energyWord(p.section.energy)]);
    return ids
      .map(lookById)
      .filter((l) => !(state.autopilot.style === "no-strobe" && l.kind === "strobe"))
      .filter((l) => l.groups.some((g) => state.autopilot.groups[g]));
  }

  const autopilotNow = (): Look[] => {
    const cur = currentSection();
    return state.autopilot.on && autopilotArmed() && cur && !covered(cur) ? autopilotLooks(cur) : [];
  };

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

  /** Keep a pick: it becomes normal show clips that you own. */
  function keepPick(p: PlacedSection) {
    const looks = autopilotLooks(p);
    if (!looks.length) return;
    const ids = looks.map(() => nextId++);
    set("clips", (cs) => [
      ...cs,
      ...looks.map((l, i) => ({ id: ids[i], track: trackFor(l), lookId: l.id, start: p.start, end: p.end, origin: "kept" as const })),
    ]);
    record(`Keep ${p.section.title}`, () => set("clips", (cs) => cs.filter((c) => !ids.includes(c.id))));
  }

  // ---- decks and the set ---------------------------------------------
  /** The song on air sits on deck A when its place in the set is even (song 2 is on A). */
  function decks(): Deck[] {
    const cur = currentSong();
    if (!cur) return [];
    const next = SONGS[(cur.index + 1) % SONGS.length];
    const onAirLetter: Deck["letter"] = cur.index % 2 === 1 ? "A" : "B";
    const other: Deck["letter"] = onAirLetter === "A" ? "B" : "A";
    const list: Deck[] = [
      { letter: onAirLetter, song: cur.song, playing: true, onAir: true, position: now() - cur.start, length: cur.end - cur.start },
      state.cuedDeckLoaded
        ? { letter: other, song: next, playing: false, onAir: false, position: 0, length: songLength(next) }
        : { letter: other, song: null, playing: false, onAir: false, position: 0, length: 0 },
    ];
    return list.sort((a, b) => a.letter.localeCompare(b.letter));
  }

  /** Tonight's set around now, with how each song is lit. */
  function setList(): SetEntry[] {
    const cycle = Math.max(0, Math.floor((now() - SET_START) / setLength));
    let t = SET_START + cycle * setLength;
    return SONGS.map((song, index) => {
      const start = t;
      const end = (t += songLength(song));
      const inSong = state.clips.filter((c) => c.start < end && c.end > start);
      const mark: SetEntry["mark"] = inSong.some((c) => c.origin === "recorded")
        ? "recorded"
        : inSong.length
          ? "show"
          : song.analysis === "queued"
            ? "not analysed"
            : "auto";
      return { song, index, start, end, mark };
    });
  }

  // ---- manual layer --------------------------------------------------
  const runningManual = () => state.manual.filter((b) => b.end === null);
  const manualBlockFor = (lookId: string) => runningManual().find((b) => b.lookId === lookId);

  function log(lookId: string, action: ManualEvent["action"]) {
    if (!isRecording()) return;
    set("events", (e) => [...e, { at: now(), lookId, action }]);
  }

  const removeBlock = (id: number) => set("manual", (bs) => bs.filter((b) => b.id !== id));
  const endBlock = (id: number, end: number | null) =>
    set("manual", (b) => b.id === id, "end", end);

  /** Start a look on the Manual layer and keep it running. A flash in progress becomes held. */
  function start(lookId: string) {
    const running = manualBlockFor(lookId);
    if (running && running.mode === "held") return;
    let id: number;
    if (running) {
      id = running.id;
      set("manual", (b) => b.id === id, "mode", "held");
    } else {
      id = nextId++;
      set("manual", (bs) => [...bs, { id, lookId, start: now(), end: null, mode: "held" }]);
    }
    log(lookId, "start");
    record(`Start ${lookById(lookId).name}`, () => removeBlock(id));
  }

  function stop(lookId: string) {
    const running = manualBlockFor(lookId);
    if (!running) return;
    const id = running.id;
    endBlock(id, now());
    log(lookId, "stop");
    record(`Stop ${lookById(lookId).name}`, () => endBlock(id, null));
  }

  /** Flash a look while a finger is down. Not an undo step. */
  function hold(lookId: string) {
    if (manualBlockFor(lookId)) return;
    set("manual", (bs) => [...bs, { id: nextId++, lookId, start: now(), end: null, mode: "flash" }]);
    log(lookId, "flash");
  }

  /** End a flash. */
  function release(lookId: string) {
    const running = manualBlockFor(lookId);
    if (running && running.mode === "flash") endBlock(running.id, now());
  }

  /** Release the whole Manual layer, so the layers below show through again. */
  function releaseAll() {
    const ids = runningManual().map((b) => b.id);
    if (!ids.length) return;
    const t = now();
    set("manual", (b) => ids.includes(b.id), "end", t);
    record("Release Manual", () => set("manual", (b) => ids.includes(b.id), "end", null));
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
      const { look, level } = fixtureState(f);
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
        else if (name === "Strobe") x = look?.kind === "strobe" ? Math.round((state.faders.strobeRate / 100) * 255) : 0;
        else if (name === "Pan") x = Math.round((state.xy.pan / 100) * 255);
        else if (name === "Tilt") x = Math.round((state.xy.tilt / 100) * 255);
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
      { id: "sacn", universe: 3, device: "sACN, bar wall", kind: "sACN", state: "connected", fps: 40, errors: 0, detail: "Sending" },
      { id: "ddp", universe: null, device: "Pixel controller 10.0.0.40", kind: "DDP", state: "connected", fps: 60, errors: 0, detail: "Sending, 480 pixels" },
    ];
  }

  function setClockFeed(feed: ClockFeed) {
    const before = state.clockFeed;
    if (before === feed) return;
    set("clockFeed", feed);
    record("Clock", () => set("clockFeed", before));
  }

  function setTempo(bpm: number) {
    const before = state.tempo;
    set("tempo", Math.max(40, Math.min(220, Math.round(bpm * 10) / 10)));
    record("Tempo", () => set("tempo", before), "tempo");
  }

  /** The Show layer replays these looks (no undo: a replay is a performance). */
  const setReplay = (ids: string[]) => set("replay", ids);

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

  function setFader(id: FaderId, v: number) {
    const before = state.faders[id];
    set("faders", id, clamp(v));
    record("Fader", () => set("faders", id, before), `fader:${id}`);
  }

  function setXY(pan: number, tilt: number) {
    const before = { ...state.xy };
    set("xy", { pan: clamp(pan), tilt: clamp(tilt) });
    record("Movers position", () => set("xy", before), "xy");
  }

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

  // ---- derived: what each fixture does --------------------------------
  /** Top layer wins: Manual, then Show, then Autopilot. */
  function fixtureState(f: Fixture): { look: Look | null; layer: Layer | null; level: number } {
    const hand = [...runningManual()].reverse().find((b) => lookById(b.lookId).groups.includes(f.group));
    let look: Look | null = null;
    let layer: Layer | null = null;
    if (hand) {
      look = lookById(hand.lookId);
      layer = "manual";
    } else {
      const sh = [...state.replay.map(lookById), ...activeClips().map((c) => lookById(c.lookId))].find((l) =>
        l.groups.includes(f.group),
      );
      const ap = autopilotNow().find((l) => l.groups.includes(f.group));
      if (sh) {
        look = sh;
        layer = "show";
      } else if (ap) {
        look = ap;
        layer = "autopilot";
      }
    }
    const groupFader: Record<GroupId, number> = {
      movers: state.faders.movers,
      pars: state.faders.wash,
      bars: state.faders.pixels,
      laser: state.faders.laser,
    };
    if (state.identify === f.id) {
      return { look: { id: "identify", name: "Identify", colors: ["#ffffff"], groups: [f.group], kind: "strobe" }, layer: null, level: beatPhase() < 0.5 ? 1 : 0.15 };
    }
    const level = isDark() || !look ? 0 : (state.master / 100) * (groupFader[f.group] / 100);
    return { look, layer, level };
  }

  /** Groups the Manual layer covers that the Show or the Autopilot also wants. */
  function overrides() {
    const showGroups = new Set([...state.replay, ...activeClips().map((c) => c.lookId)].flatMap((id) => lookById(id).groups));
    const apGroups = new Set(autopilotNow().flatMap((l) => l.groups));
    return runningManual()
      .map((b) => ({ block: b, look: lookById(b.lookId) }))
      .map((x) => {
        const under: "Show" | "Autopilot" = x.look.groups.some((g) => showGroups.has(g)) ? "Show" : "Autopilot";
        const wanted = under === "Show" ? showGroups : apGroups;
        return { ...x, under, groups: x.look.groups.filter((g) => wanted.has(g)) };
      })
      .filter((x) => x.groups.length > 0);
  }

  /** Every group the Manual layer covers right now, with the look that covers it. */
  function handsOff() {
    return GROUPS.map((g) => {
      const b = [...runningManual()].reverse().find((x) => lookById(x.lookId).groups.includes(g.id));
      return b ? { group: g, block: b, look: lookById(b.lookId) } : null;
    }).filter((x) => x !== null);
  }

  /** How a look is running: "held" or "flash" on Manual, "show", "autopilot", or null. */
  function lookState(lookId: string): "held" | "flash" | "show" | "autopilot" | null {
    const b = manualBlockFor(lookId);
    if (b) return b.mode;
    if (activeClips().some((c) => c.lookId === lookId)) return "show";
    if (autopilotNow().some((l) => l.id === lookId)) return "autopilot";
    return null;
  }

  // ---- lanes for the timetable and the Now strip ---------------------
  function lanes(from: number, to: number): Lane[] {
    const t = now();
    const manualBlocks = state.manual
      .filter((b) => (b.end ?? t) > from && b.start < to)
      .map((b) => ({
        id: `manual-${b.id}`,
        title: lookById(b.lookId).name,
        sub: hhmm(b.start),
        start: b.start,
        end: b.end,
        tail: b.end === null ? (b.mode === "flash" ? "flash" : "holding") : undefined,
      }));
    const handGroups = handsOff().map((h) => h.group.name);
    const cur = currentSection();
    const spans: { start: number; end: number; clips: ShowClip[] }[] = [];
    for (const c of [...state.clips].sort((a, b) => a.start - b.start)) {
      const last = spans[spans.length - 1];
      if (last && c.start <= last.end) {
        last.end = Math.max(last.end, c.end);
        last.clips.push(c);
      } else spans.push({ start: c.start, end: c.end, clips: [c] });
    }
    const spanTitle = (sp: (typeof spans)[number]) =>
      sp.clips.every((c) => c.origin === "recorded") ? "Recorded" : (sectionAt(sp.start + 0.001)?.section.title ?? "Show");
    const showBlocks = spans
      .filter((sp) => sp.end > from && sp.start < to)
      .map((sp) => ({ id: `show-${sp.start}`, title: spanTitle(sp), sub: hhmm(sp.start), start: sp.start, end: sp.end }));
    const showNow = spans.find((sp) => sp.start <= t && t < sp.end);
    const showNext = spans.find((sp) => sp.start > t);
    const apBlocks = sectionsBetween(from, to).filter((p) => !covered(p)).map((p) => ({
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
        status: handGroups.length ? `covers ${handGroups.join(", ")}` : "nothing held",
        blocks: manualBlocks,
      },
      {
        layer: "show",
        title: "Show",
        on: !!showNow || state.replay.length > 0,
        chip: showNow || state.replay.length ? "playing" : "idle",
        status: state.replay.length
          ? `replays ${state.replay.map((id) => lookById(id).name).join(", ")}`
          : showNow
            ? spanTitle(showNow) : showNext ? `next at ${hhmm(showNext.start)}` : "nothing planned",
        blocks: showBlocks,
        empty: "No show loaded",
      },
      {
        layer: "autopilot",
        title: "Autopilot",
        on: apPlaying,
        chip: !state.autopilot.on ? "off" : !autopilotArmed() ? "waiting" : apPlaying ? "playing" : "idle",
        status: !state.autopilot.on
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
    moveClip,
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
    overrides,
    handsOff,
    lanes,
    sectionsBetween,
    currentSection,
    nextSection,
    currentSong,
    currentSongSections,
    autopilotLooks,
    pickReason,
    covered,
    activeClips,
    decks,
    setList,
    autopilotNow,
    get rig() {
      return state.rig;
    },
    sectionLooksOf,
    setSectionLooks,
    fillFromStyle,
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
    attributes,
    channelValues,
    holdChannel,
    releaseChannel,
    releaseAllChannels,
    outputs,
    setClockFeed,
    setTempo,
    setReplay,
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
