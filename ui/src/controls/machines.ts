// The state machines of the controls that have more than two states (docs/coding.md, rule 3; issue #102).
// One type lists the states, one table holds every pair of state and event. Nothing here knows the engine:
// a transition names the commands to send, as data.
import type { CommandName } from "./commands.ts";

/** Where a pair of state and event leads, and what is sent on the way. */
export interface Step<S extends string> {
  to: S;
  sends?: readonly CommandName[];
  /** What happens, when the new state alone does not say it. */
  note?: string;
}

export interface Machine<S extends string, E extends string> {
  title: string;
  start: S;
  /** Every state, with what it means on screen. */
  states: Record<S, string>;
  /** Every event, with what causes it. */
  events: Record<E, string>;
  /** Every pair. A pair that changes nothing leads to its own state. */
  table: Record<S, Record<E, Step<S>>>;
}

/** The one transition function of every machine: state and event in, new state and commands out. */
export function step<S extends string, E extends string>(machine: Machine<S, E>, state: S, event: E): Step<S> {
  return machine.table[state][event];
}

/** Runs a list of events from the start. The states passed through and every command sent, in order. */
export function walk<S extends string, E extends string>(machine: Machine<S, E>, events: readonly E[]): { states: S[]; sent: CommandName[] } {
  const states: S[] = [machine.start];
  const sent: CommandName[] = [];
  for (const event of events) {
    const next = step(machine, states[states.length - 1] ?? machine.start, event);
    states.push(next.to);
    sent.push(...(next.sends ?? []));
  }
  return { states, sent };
}

const stay = <S extends string>(to: S): Step<S> => ({ to });
const go = <S extends string>(to: S, sends: readonly CommandName[] = [], note?: string): Step<S> =>
  note === undefined ? { to, sends } : { to, sends, note };

// ---- a look pad

type PadState = "off" | "flash" | "on" | "onDown" | "kept";
type PadEvent = "down" | "upShort" | "upLong" | "release";

/**
 * A look pad on the Console. It fires on finger down, so it feels instant: a short press latches,
 * a long hold was a flash. Who else runs the look (show, autopilot) is the engine's report and
 * changes the pad's colour, not this machine: for the hand, a pad run by others is `off`.
 */
export const pad: Machine<PadState, PadEvent> = {
  title: "Look pad",
  start: "off",
  states: {
    off: "Not run by hand. Dark body, or the colour of who runs it (show, autopilot)",
    flash: "Finger down on a pad that was off: the look runs, pink, with the paper frame",
    on: "Runs by hand, latched: pink, NOW flag",
    onDown: "Finger down on a pad that runs: nothing changes yet",
    kept: "Pad off, but what it touched is still yours: pink outline and a 'held' tag",
  },
  events: {
    down: "finger, mouse or mapped pad goes down",
    upShort: "it comes up within 350 ms",
    upLong: "it comes up later",
    release: "Release for this look, or Release all",
  },
  table: {
    off: { down: go("flash", ["StartLook"]), upShort: stay("off"), upLong: stay("off"), release: stay("off") },
    flash: {
      down: stay("flash"),
      upShort: go("on", [], "it was a press: the look stays on"),
      upLong: go("off", ["EndFlash"], "it was a flash: nothing stays held"),
      release: go("off", [], "released under the finger: lifting it does nothing more"),
    },
    on: { down: stay("onDown"), upShort: stay("on"), upLong: stay("on"), release: go("off") },
    onDown: {
      down: stay("onDown"),
      upShort: go("kept", ["StopLook"], "pad off. With 'manual control ends when the pad stops' it goes to off"),
      upLong: go("on", [], "a long hold on a running pad changes nothing"),
      release: go("off"),
    },
    kept: { down: go("flash", ["StartLook"]), upShort: stay("kept"), upLong: stay("kept"), release: go("off") },
  },
};

// ---- Try on lights

type TryState = "off" | "flash" | "on" | "onDown";

/** The Try pad of the Looks editor: a pad whose look gives back what it touched when it stops. Nothing is kept. */
export const tryPad: Machine<TryState, PadEvent> = {
  title: "Try on lights",
  start: "off",
  states: {
    off: "The look is not tried",
    flash: "Finger down: the look runs",
    on: "The look runs by hand until the next press",
    onDown: "Finger down while it runs: nothing changes yet",
  },
  events: pad.events,
  table: {
    off: { down: go("flash", ["StartLook"]), upShort: stay("off"), upLong: stay("off"), release: stay("off") },
    flash: { down: stay("flash"), upShort: go("on"), upLong: go("off", ["EndFlash"]), release: go("off") },
    on: { down: stay("onDown"), upShort: stay("on"), upLong: stay("on"), release: go("off") },
    onDown: { down: stay("onDown"), upShort: go("off", ["EndFlash"], "a try gives back what it touched"), upLong: stay("on"), release: go("off") },
  },
};

// ---- Blackout

type BlackoutState = "off" | "held" | "latched";
type BlackoutEvent = "press" | "spaceDown" | "spaceUp";

/** Blackout: the button latches, Space is dark only while it is down. Leaving the window counts as Space up. */
export const blackout: Machine<BlackoutState, BlackoutEvent> = {
  title: "Blackout",
  start: "off",
  states: {
    off: "Lights follow the app. Red outline, 'Blackout'",
    held: "Dark while Space is down. Red fill, 'Blackout on'",
    latched: "Dark until the next press. Red fill, 'Blackout on'",
  },
  events: {
    press: "the Blackout button or its mapped control",
    spaceDown: "Space goes down, outside a text field",
    spaceUp: "Space comes up, or the window loses the keyboard",
  },
  table: {
    off: { press: go("latched", ["Blackout"]), spaceDown: go("held", ["Blackout"]), spaceUp: stay("off") },
    held: { press: go("latched", [], "already dark: it stays dark when Space comes up"), spaceDown: stay("held"), spaceUp: go("off", ["Blackout"]) },
    latched: { press: go("off", ["Blackout"]), spaceDown: stay("latched"), spaceUp: stay("latched") },
  },
};

// ---- one of the four indicators beside the LIVE switch

type IndicatorState = "ready" | "on" | "off" | "offLive";
type IndicatorEvent = "press" | "liveOn" | "liveOff";

/**
 * DJ link, Recording, Autopilot, Lock: each is on or turned off, and in effect only while LIVE is on.
 * A press sends the indicator's own command (SetFollowDj, SetRecording, SetAutopilot, SetLock).
 * The Autopilot button in the Running panel is this same state, shown a second time.
 */
export const indicator: Machine<IndicatorState, IndicatorEvent> = {
  title: "LIVE indicator",
  start: "ready",
  states: {
    ready: "Mute lamp: starts with LIVE",
    on: "Paper lamp: in effect now. Recording is red",
    off: "Empty lamp, word struck through: turned off",
    offLive: "The same look as off, while LIVE is on",
  },
  events: {
    press: "the indicator, or its mapped control",
    liveOn: "LIVE turns on",
    liveOff: "LIVE turns off",
  },
  table: {
    ready: { press: go("off", [], "its own command, off"), liveOn: stay("on"), liveOff: stay("ready") },
    on: { press: go("offLive", [], "its own command, off"), liveOn: stay("on"), liveOff: stay("ready") },
    off: { press: go("ready", [], "its own command, on"), liveOn: stay("offLive"), liveOff: stay("off") },
    offLive: { press: go("on", [], "its own command, on"), liveOn: stay("offLive"), liveOff: stay("off") },
  },
};

// ---- Learn

type LearnState = "off" | "pick" | "wait";
type LearnEvent = "press" | "pick" | "moved" | "cancel";

/** Learn: pick a control on screen, then move a control on the hardware. */
export const learn: Machine<LearnState, LearnEvent> = {
  title: "Learn",
  start: "off",
  states: {
    off: "Every control does what it does",
    pick: "Step 1 of 2: the next press on a control picks it and does not fire it",
    wait: "Step 2 of 2: waiting for a hardware control to move",
  },
  events: {
    press: "the Learn button",
    pick: "a press on a control that can be learned",
    moved: "a control on the hardware moves",
    cancel: "Cancel in the Learn bar, or Escape",
  },
  table: {
    off: { press: stay("pick"), pick: stay("off"), moved: stay("off"), cancel: stay("off") },
    pick: { press: stay("off"), pick: go("wait", ["StartLearn"]), moved: stay("pick"), cancel: stay("off") },
    wait: {
      press: go("off", ["CancelLearn"]),
      pick: stay("wait"),
      moved: go("off", [], "mapped by the engine. The bar says what was mapped, for four seconds"),
      cancel: go("off", ["CancelLearn"]),
    },
  },
};

// ---- the Console layout

type SurfaceState = "playing" | "editing" | "locked";
type SurfaceEvent = "edit" | "done" | "lockOn" | "lockOff";

/** The Console layout: played, arranged, or locked while LIVE is on. */
export const surface: Machine<SurfaceState, SurfaceEvent> = {
  title: "Console layout",
  start: "playing",
  states: {
    playing: "Widgets play. 'Edit surface' is there",
    editing: "Widgets are arranged and do not play. Grid, handles and the drawer show. 'Done editing' is there",
    locked: "Widgets play. 'Edit surface' is off and says 'Locked while LIVE is on'",
  },
  events: {
    edit: "Edit surface",
    done: "Done editing",
    lockOn: "the lock comes into effect: LIVE on with Lock on",
    lockOff: "the lock ends: LIVE off, or Lock turned off",
  },
  table: {
    playing: { edit: stay("editing"), done: stay("playing"), lockOn: stay("locked"), lockOff: stay("playing") },
    editing: { edit: stay("editing"), done: stay("playing"), lockOn: go("locked", [], "an edit in progress ends. What was placed stays"), lockOff: stay("editing") },
    locked: { edit: stay("locked"), done: stay("locked"), lockOn: stay("locked"), lockOff: stay("playing") },
  },
};

// ---- Add lights

type AddState = "find" | "mode" | "address" | "flash";
type AddEvent = "pick" | "next" | "back" | "yes" | "no" | "startOver";

/** The guided add on Patch, in place. "Leave for now" hides it and keeps the step. */
export const addLights: Machine<AddState, AddEvent> = {
  title: "Add lights",
  start: "find",
  states: {
    find: "Step 1 of 4: find the light in the library",
    mode: "Step 2 of 4: pick its mode",
    address: "Step 3 of 4: a free address is proposed, with the switches to set",
    flash: "Step 4 of 4: the new light flashes. Is that this one?",
  },
  events: {
    pick: "a model in step 1, a mode in step 2",
    next: "Next",
    back: "Back",
    yes: "Yes, that is this one",
    no: "Nothing flashed",
    startOver: "Start over",
  },
  table: {
    find: { pick: stay("mode"), next: stay("find"), back: stay("find"), yes: stay("find"), no: stay("find"), startOver: stay("find") },
    mode: { pick: stay("mode"), next: stay("address"), back: stay("find"), yes: stay("mode"), no: stay("mode"), startOver: stay("find") },
    address: {
      pick: stay("address"),
      next: go("flash", ["Identify"], "off while the address clashes"),
      back: stay("mode"),
      yes: stay("address"),
      no: stay("address"),
      startOver: stay("find"),
    },
    flash: {
      pick: stay("flash"),
      next: stay("flash"),
      back: go("address", ["Identify"], "the flashing stops"),
      yes: go("find", ["AddFixture", "Identify"], "the light joins the patch and is selected. The flow closes"),
      no: go("address", ["Identify"], "with a hint: check the cable, the mode and the switches"),
      startOver: go("find", ["Identify"]),
    },
  },
};

// ---- dragging

type DragState = "idle" | "armed" | "dragging";
type DragEvent = "down" | "move" | "up" | "cancel";

/**
 * Every drag in the app: a look onto a group, a widget, a light in the universe grid, an item of a sequence,
 * the edge of a section. Below 8 px it was a press. Escape puts it back.
 */
export const drag: Machine<DragState, DragEvent> = {
  title: "Drag",
  start: "idle",
  states: {
    idle: "Nothing is dragged",
    armed: "Finger down, moved less than 8 px: still a press",
    dragging: "It follows the finger. Where it would land is marked",
  },
  events: {
    down: "finger or mouse goes down on it",
    move: "it moves 8 px or more",
    up: "it comes up",
    cancel: "Escape, or the pointer is lost",
  },
  table: {
    idle: { down: stay("armed"), move: stay("idle"), up: stay("idle"), cancel: stay("idle") },
    armed: { down: stay("armed"), move: stay("dragging"), up: go("idle", [], "it was a press: the press fires"), cancel: stay("idle") },
    dragging: {
      down: stay("dragging"),
      move: stay("dragging"),
      up: go("idle", [], "the control's command is sent once, with where it landed: one step for Undo"),
      cancel: go("idle", [], "it goes back where it was. Nothing is sent"),
    },
  },
};

export const MACHINES = { pad, tryPad, blackout, indicator, learn, surface, addLights, drag } as const;
export type MachineName = keyof typeof MACHINES;
