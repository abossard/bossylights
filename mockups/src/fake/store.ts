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

/** A layer of the engine. Three to start with; you can add more. Top first. */
export interface LayerDef {
  id: string;
  name: string;
  kind: "manual" | "show" | "autopilot" | "extra";
  /** colour of an added layer; the first three use their tokens */
  color?: string;
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

/** A recording pass: one layer of a song's show. */
export interface SongLayer {
  id: string;
  name: string;
  muted: boolean;
  /** still recording: drawn in red */
  recording: boolean;
  events: RecEvent[];
}

export interface Lane {
  /** the layer id */
  layer: string;
  /** colour of an added layer */
  color?: string;
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
  /** Everything mapped with Learn, anywhere in the app. */
  mappings: Mapping[];
  /** Steps, sequences and patterns, by id. */
  library: Record<string, Item>;
  /** The Console's pad banks, by page. */
  pads: Record<string, string[]>;
  /** Looks the Show layer replays right now from a recording (How it works). */
  replay: string[];
  /** false: the cued deck has nothing loaded (an edge state to look at) */
  cuedDeckLoaded: boolean;
  blackout: boolean;
  blackoutHold: boolean;
  faders: Record<FaderId, number>;
  xy: { pan: number; tilt: number };
  manual: ManualBlock[];
  /** The engine's layers, top first. */
  layers: LayerDef[];
  /** Sections per song, editable. */
  songSections: Record<string, SongSection[]>;
  /** Recording passes per song, newest first. */
  songLayers: Record<string, SongLayer[]>;
  /** A song's base show layer, muted. */
  baseMuted: Record<string, boolean>;
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
    sectionLooks: Object.fromEntries([
      ...SONGS[0].sections.map((x, i) => [`${SONGS[0].id}-${i}`, x.looks]),
      ...SONGS[1].sections.map((x, i) => [`${SONGS[1].id}-${i}`, i === 1 ? [] : x.looks]),
    ]),
    reviewed: [],
    // three channels held by hand: PAR 1 red, PAR 5 dimmer, Mover R1 pan
    held: { 66: 255, 93: 180, 33: 96 },
    clockFeed: "vdj",
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
      { id: 1, lookId: "red-alert", start: 22 * 3600 + 40 * 60 + 30, end: 22 * 3600 + 42 * 60 + 40, mode: "held" },
      { id: 2, lookId: "strobe", start: 22 * 3600 + 43 * 60 + 30, end: null, mode: "held" },
    ],
    layers: [
      { id: "manual", name: "Manual", kind: "manual" },
      { id: "show", name: "Show", kind: "show" },
      { id: "autopilot", name: "Autopilot", kind: "autopilot" },
    ],
    songSections: Object.fromEntries(
      [...SONGS, ...LIBRARY_ONLY].map((song) => [song.id, song.sections.map((x, i) => ({ ...x, id: `${song.id}-${i}` }))]),
    ),
    // a pass recorded earlier today on Glass Hours, beside its base layer
    songLayers: {
      "glass-hours": [
        {
          id: "rec-1",
          name: "Recorded today 21:58",
          muted: false,
          recording: false,
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
    baseMuted: {},
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
  /** A recorded layer's blocks: start to stop, or a flash. */
  function recBlocks(l: SongLayer, songLen: number, pos = songPos()) {
    const out: { lookId: string; start: number; end: number }[] = [];
    const open: Record<string, number> = {};
    for (const ev of [...l.events].sort((x, y) => x.at - y.at)) {
      if (ev.kind === "start" && ev.lookId) open[ev.lookId] = ev.at;
      if (ev.kind === "stop" && ev.lookId && open[ev.lookId] !== undefined) {
        out.push({ lookId: ev.lookId, start: open[ev.lookId], end: ev.at });
        delete open[ev.lookId];
      }
      if (ev.kind === "flash" && ev.lookId) out.push({ lookId: ev.lookId, start: ev.at, end: ev.at + (ev.dur ?? 1) });
    }
    // still held while recording: it runs up to now
    for (const [id, at] of Object.entries(open)) out.push({ lookId: id, start: at, end: l.recording ? Math.max(at + 1, pos) : songLen });
    return out;
  }
  /** What the Show layer plays now: the section's blocks and every recorded layer that is not muted. */
  function showLooksNow(): string[] {
    const cur = currentSection();
    if (!cur) return [...state.replay];
    const pos = now() - cur.songStart;
    const base = state.baseMuted[cur.song.id] ? [] : sectionLooksOf(cur.song, cur.index);
    const rec = (state.songLayers[cur.song.id] ?? [])
      .filter((l) => !l.muted && !l.recording)
      .flatMap((l) => recBlocks(l, songLength(cur.song)).filter((x) => x.start <= pos && pos < x.end).map((x) => x.lookId));
    return [...new Set([...state.replay, ...rec, ...base])];
  }
  /** A section the show already covers: the autopilot leaves it alone. */
  const covered = (p: PlacedSection) => !state.baseMuted[p.song.id] && sectionLooksOf(p.song, p.index).length > 0;

  /** One undo step for any change to a song's show. */
  function editSong(song: Song, label: string, fn: () => void) {
    const before = {
      sections: JSON.parse(JSON.stringify(sectionsOf(song))) as SongSection[],
      layers: JSON.parse(JSON.stringify(state.songLayers[song.id] ?? [])) as SongLayer[],
      looks: { ...state.sectionLooks },
      muted: !!state.baseMuted[song.id],
    };
    fn();
    record(label, () => {
      set("songSections", song.id, before.sections);
      set("songLayers", song.id, before.layers);
      set("sectionLooks", reconcile(before.looks));
      set("baseMuted", song.id, before.muted);
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

  const songLayersOf = (song: Song) => state.songLayers[song.id] ?? [];
  function setLayerMuted(song: Song, layerId: string, muted: boolean) {
    editSong(song, muted ? "Mute layer" : "Unmute layer", () => {
      if (layerId === "base") set("baseMuted", song.id, muted);
      else set("songLayers", song.id, (l) => l.id === layerId, "muted", muted);
    });
  }
  /** Merge a recorded layer into the one below it (an older pass, or the base layer). */
  function mergeLayerDown(song: Song, layerId: string) {
    const layers = songLayersOf(song);
    const i = layers.findIndex((l) => l.id === layerId);
    if (i < 0) return;
    const l = layers[i];
    editSong(song, `Merge ${l.name}`, () => {
      const below = layers[i + 1];
      if (below) {
        set("songLayers", song.id, i + 1, "events", [...below.events, ...l.events].sort((x, y) => x.at - y.at));
      } else {
        // into the base layer: every look started in a section joins that section
        const secs = sectionsOf(song);
        let t = 0;
        const bounds = secs.map((x) => [t, (t += x.length)]);
        for (const ev of l.events) {
          if ((ev.kind !== "start" && ev.kind !== "flash") || !ev.lookId) continue;
          const k = bounds.findIndex(([a, b2]) => ev.at >= a && ev.at < b2);
          if (k < 0) continue;
          const ids = state.sectionLooks[secs[k].id] ?? [];
          if (!ids.includes(ev.lookId)) set("sectionLooks", secs[k].id, [...ids, ev.lookId]);
        }
      }
      set("songLayers", song.id, (ls) => ls.filter((x) => x.id !== layerId));
    });
  }
  function deleteLayer(song: Song, layerId: string) {
    editSong(song, "Delete layer", () => set("songLayers", song.id, (ls) => ls.filter((x) => x.id !== layerId)));
  }

  // recording writes into the show of the song that plays, as a new layer
  let recSeq = 2;
  function recordEvent(ev: Omit<RecEvent, "at"> & { at?: number }) {
    if (!isRecording()) return;
    const cur = currentSong();
    if (!cur) return;
    const id = cur.song.id;
    const layers = state.songLayers[id] ?? [];
    let l = layers.find((x) => x.recording);
    if (!l) {
      l = { id: `rec-${recSeq++}`, name: `Recording ${hhmm(now())}`, muted: false, recording: true, events: [] };
      set("songLayers", id, [l, ...layers]);
    }
    const lid = l.id;
    set("songLayers", id, (x) => x.id === lid, "events", (es) => [...es, { ...ev, at: ev.at ?? now() - cur.start }]);
  }
  // a pass ends when recording stops or the song changes
  createEffect(() => {
    const rec = isRecording();
    const playing = currentSong()?.song.id;
    for (const [songId, layers] of Object.entries(state.songLayers)) {
      if (!rec || songId !== playing)
        layers.forEach((l, i) => l.recording && set("songLayers", songId, i, { recording: false, name: l.name.replace("Recording", "Recorded") }));
    }
  });

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
  const sectionKey = (song: Song, index: number) => sectionsOf(song)[index]?.id ?? `${song.id}-${index}`;
  /** The looks a section of the song's show holds (its base layer). */
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

  /** Fill every empty section from the autopilot's style. One undo step. */
  function fillFromStyle(song: Song) {
    const before = { ...state.sectionLooks };
    let n = 0;
    sectionsOf(song).forEach((sec, i) => {
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
    // the autopilot's own picks: the analysis suggests some, else by energy
    const ids = state.autopilot.swaps[p.key] ?? (p.section.looks.length ? p.section.looks : FALLBACK[energyWord(p.section.energy)]);
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

  // ---- manual layer --------------------------------------------------
  const runningManual = () => state.manual.filter((b) => b.end === null);
  const manualBlockFor = (lookId: string) => runningManual().find((b) => b.lookId === lookId);

  function log(lookId: string, action: ManualEvent["action"]) {
    if (!isRecording()) return;
    set("events", (e) => [...e, { at: now(), lookId, action }]);
    // a flash is written when it ends, with its length
    if (action !== "flash") recordEvent({ kind: action, lookId });
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
    if (!running || running.mode !== "flash") return;
    endBlock(running.id, now());
    const cur = currentSong();
    if (cur) recordEvent({ kind: "flash", lookId, at: running.start - cur.start, dur: now() - running.start });
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

  /** The Show layer replays these looks (no undo: a replay is a performance). */
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
  const allGroups = () => [...new Set(items().flatMap((i) => i.groups))].sort();

  // every library item plays like a look on the layers
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
    editItem(id, `Add to ${g}`, (it) => void it.groups.push(g));
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
    const l = (state.songLayers[cur.song.id] ?? []).find((x) => x.recording);
    const last = l?.events[l.events.length - 1];
    const at = now() - cur.start;
    if (l && last?.kind === "fader" && last.fader === fader && at - last.at < 1.5) {
      set("songLayers", cur.song.id, (x) => x.id === l.id, "events", l.events.length - 1, { at, value });
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
      const sh = showLooksNow().map(lookById).find((l) => l.groups.includes(f.group));
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
    const showGroups = new Set(showLooksNow().flatMap((id) => lookById(id).groups));
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
    if (showLooksNow().includes(lookId)) return "show";
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
    const builtIn: Record<string, Lane> = {
      manual: {
        layer: "manual",
        title: "Manual",
        on: handGroups.length > 0,
        chip: handGroups.length ? "holding" : "idle",
        status: handGroups.length ? `covers ${handGroups.join(", ")}` : "nothing held",
        blocks: manualBlocks,
      },
      show: {
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
      autopilot: {
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
    };
    return state.layers.map(
      (l): Lane =>
        builtIn[l.kind] ?? { layer: l.id, title: l.name, color: l.color, on: false, chip: "idle", status: "nothing on it yet", blocks: [], empty: "Nothing on this layer yet" },
    );
  }

  const EXTRA_COLOURS = ["#9d7bff", "#ff9a3d", "#3ddc97", "#c6d94a"];
  /** Add a layer: it goes under Manual, with its own colour. */
  function addLayer() {
    const n = state.layers.filter((l) => l.kind === "extra").length;
    const id = `layer-${Date.now().toString(36)}`;
    const layer: LayerDef = { id, name: `Layer ${state.layers.length + 1}`, kind: "extra", color: EXTRA_COLOURS[n % EXTRA_COLOURS.length] };
    set("layers", (ls) => [ls[0], layer, ...ls.slice(1)]);
    record(`Add ${layer.name}`, () => set("layers", (ls) => ls.filter((l) => l.id !== id)));
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
    addLayer,
    sectionsOf,
    songLength,
    songPos,
    recBlocks,
    showLooksNow,
    songLayersOf,
    setLayerMuted,
    mergeLayerDown,
    deleteLayer,
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
    decks,
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
