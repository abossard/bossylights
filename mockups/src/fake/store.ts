// One fake state store drives every screen. A pad pressed on the Console shows up
// everywhere that reads this store.
import { createRoot, createSignal } from "solid-js";
import { createStore, produce } from "solid-js/store";
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

/** A section of a song placed on the show clock. */
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
  /** Label on the dashed tail of a block that is still running: "held" or "flash". */
  tail?: string;
}

export interface Lane {
  layer: Layer;
  title: string;
  on: boolean;
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
  blackout: boolean;
  blackoutHold: boolean;
  faders: Record<FaderId, number>;
  xy: { pan: number; tilt: number };
  manual: ManualBlock[];
  show: { on: boolean };
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
    blackout: false,
    blackoutHold: false,
    faders: { wash: 70, movers: 60, pixels: 45, strobeRate: 30, laser: 0 },
    xy: { pan: 38, tilt: 62 },
    manual: [
      { id: 1, lookId: "red-alert", start: 22 * 3600 + 40 * 60 + 30, end: 22 * 3600 + 42 * 60 + 40, mode: "held" },
      { id: 2, lookId: "strobe", start: 22 * 3600 + 43 * 60 + 30, end: null, mode: "held" },
    ],
    show: { on: false },
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

  let nextId = 3;

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

  /** What the autopilot plays in a section, after style, groups and skips. */
  function autopilotLooks(p: PlacedSection): Look[] {
    const ids = state.autopilot.swaps[p.key] ?? p.section.looks;
    return ids
      .map(lookById)
      .filter((l) => !(state.autopilot.style === "no-strobe" && l.kind === "strobe"))
      .filter((l) => l.groups.some((g) => state.autopilot.groups[g]));
  }

  const autopilotNow = (): Look[] => {
    const cur = currentSection();
    return state.autopilot.on && autopilotArmed() && cur ? autopilotLooks(cur) : [];
  };

  // ---- manual layer --------------------------------------------------
  const runningManual = () => state.manual.filter((b) => b.end === null);
  const manualBlockFor = (lookId: string) => runningManual().find((b) => b.lookId === lookId);

  function log(lookId: string, action: ManualEvent["action"]) {
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
      const ap = autopilotNow().find((l) => l.groups.includes(f.group));
      if (ap) {
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
    const level = isDark() || !look ? 0 : (state.master / 100) * (groupFader[f.group] / 100);
    return { look, layer, level };
  }

  /** Groups the Manual layer covers that the Autopilot also wants. */
  function overrides() {
    const apGroups = new Set(autopilotNow().flatMap((l) => l.groups));
    return runningManual()
      .map((b) => ({ block: b, look: lookById(b.lookId) }))
      .map((x) => ({ ...x, groups: x.look.groups.filter((g) => apGroups.has(g)) }))
      .filter((x) => x.groups.length > 0);
  }

  /** Every group the Manual layer covers right now, with the look that covers it. */
  function handsOff() {
    return GROUPS.map((g) => {
      const b = [...runningManual()].reverse().find((x) => lookById(x.lookId).groups.includes(g.id));
      return b ? { group: g, block: b, look: lookById(b.lookId) } : null;
    }).filter((x) => x !== null);
  }

  /** How a look is running: "held" or "flash" on Manual, "autopilot", or null. */
  function lookState(lookId: string): "held" | "flash" | "autopilot" | null {
    const b = manualBlockFor(lookId);
    if (b) return b.mode;
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
        tail: b.end === null ? (b.mode === "flash" ? "flash" : "held") : undefined,
      }));
    const covered = handsOff().map((h) => h.group.name);
    const cur = currentSection();
    const apBlocks = sectionsBetween(from, to).map((p) => ({
      id: `ap-${p.key}`,
      title: p.section.title,
      sub: p.key === cur?.key ? `${hhmm(p.start)}\n${mmss(p.end - t)} left` : hhmm(p.start),
      start: p.start,
      end: p.end,
    }));
    return [
      {
        layer: "manual",
        title: "Manual",
        on: covered.length > 0,
        status: covered.length ? `covers ${covered.join(", ")}` : "idle",
        blocks: manualBlocks,
      },
      {
        layer: "show",
        title: "Show",
        on: state.show.on,
        status: isRecording() ? "recording" : "idle",
        blocks: [],
        empty: "No show loaded",
      },
      {
        layer: "autopilot",
        title: "Autopilot",
        on: state.autopilot.on && autopilotArmed(),
        status: !state.autopilot.on
          ? "off"
          : !state.night.live
            ? "waits for LIVE"
            : !autopilotArmed()
              ? "held back at LIVE"
              : cur
                ? `playing ${cur.section.title}`
                : "idle",
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
    setLive,
    toggleIndicator,
    setFader,
    setXY,
    setAutopilot,
    toggleGroup,
    skipLook,
    moreLikeThis,
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
    autopilotNow,
    rig: RIG,
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
