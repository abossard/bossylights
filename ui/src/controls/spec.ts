// What a row of the behaviour table holds, and the usual answers by kind of control (issue #102).
// A row is data. `control()` fills in what a kind always answers, a row states only where it differs.
import type { CommandName } from "./commands.ts";
import type { MachineName } from "./machines.ts";
import { works, off, gone, type Cell, type Gesture, type SituationId } from "./situations.ts";

export type Screen = "Top bar" | "Console" | "Songs" | "Looks" | "Patch" | "DMX" | "In/Out" | "How it works" | "Modules" | "Menu bar";

/** The primitives of `ui/src/kit`, named by what they are for, and the kinds of each. */
export const PRIMITIVES = {
  Button: ["action", "toggle"],
  Pad: [],
  Fader: [],
  XY: ["position", "colour"],
  Picker: ["tabs", "segments", "cards", "pictures", "list"],
  Field: ["text", "search", "steps"],
  Status: ["lamp", "pill", "flag", "value"],
  Item: ["row", "tile", "cell", "entry", "chip", "strip"],
  Block: ["bar", "frame"],
  Handle: ["edge", "corner"],
  Preview: ["matrix"],
} as const;

export type Primitive = keyof typeof PRIMITIVES;

/** What each primitive is for: the question the operator has when reaching for it. */
export const PRIMITIVE_FOR: Record<Primitive, string> = {
  Button: "Do it, or switch it",
  Pad: "Play a look",
  Fader: "Set one value",
  XY: "Set two values at once",
  Picker: "Choose one of a few",
  Field: "Give a value in words or in steps",
  Status: "Read a state. Never pressed: no control is one, many carry one",
  Item: "A thing where it lives: pick it, open it, take it out",
  Block: "A thing placed in time or on a grid: arrange it",
  Handle: "The edge of a thing: only dragged",
  Preview: "See it before it plays. Never pressed: no control is one, an option of a picker can carry one",
};

/** `Primitive.kind`, or the primitive alone when it has one kind. */
export const kitNames = (primitive: Primitive): string[] => (PRIMITIVES[primitive].length ? PRIMITIVES[primitive].map((kind) => `${primitive}.${kind}`) : [primitive]);

type Named<P extends Primitive> = (typeof PRIMITIVES)[P] extends readonly [] ? P : `${P}.${(typeof PRIMITIVES)[P][number]}`;

/** The primitives that are only read: no control is one. */
type ReadOnly = "Status" | "Preview";

/** What draws a control: a primitive and its kind. Where it lives on screen is `screen`, never the name of a component. */
export type Kit = { [P in Exclude<Primitive, ReadOnly>]: Named<P> }[Exclude<Primitive, ReadOnly>];

export const primitiveOf = (kit: Kit): Primitive => (kit.split(".")[0] ?? kit) as Primitive;
/** Empty for a primitive of one kind. */
export const kindOf = (kit: Kit): string => kit.split(".")[1] ?? "";

/**
 * | kind    | acts on                                               |
 * |---------|-------------------------------------------------------|
 * | play    | the lights, now, by hand                              |
 * | edit    | the show file: looks, songs, patch, layout, mappings  |
 * | setting | how the engine behaves from now on                    |
 * | view    | what this screen shows, nothing else                  |
 */
export type Kind = "play" | "edit" | "setting" | "view";

export interface Answer {
  yes: boolean;
  why: string;
}

/** The "Function first" test of DESIGN.md. `change` and `remove` say what happens to it. */
export interface Verdict {
  verdict: "keep" | "change" | "remove";
  why: string;
}

export interface Feedback {
  /** What changes on screen, and when. */
  screen: string;
  /** What a mapped controller shows. */
  controller: string;
}

export interface Control {
  /** Its stable name: screen, part, control. */
  id: string;
  /** What the operator calls it. */
  name: string;
  screen: Screen;
  /** The part of the screen it sits in. */
  part: string;
  /** The elements of the mockups it stands for: keys of `inventory.json`. */
  from: readonly string[];
  /** Where it was decided, when no mockup shows it: an issue. */
  decided: string;
  kit: Kit;
  kind: Kind;
  /** What it does, in one line. */
  does: string;
  /** The commands it sends. None: it changes only `ui`. */
  sends: readonly CommandName[];
  /** The state of the screen it changes, where it has one. */
  ui: string;
  gestures: Record<Gesture, string>;
  cells: Record<SituationId, Cell>;
  feedback: Feedback;
  undo: Answer;
  learn: Answer;
  first: Verdict;
  /** Its state machine, when it has more than two states. */
  machine?: MachineName;
  /** The states it can show, for the frames of its catalog page. */
  states: readonly string[];
}

export const yes = (why: string): Answer => ({ yes: true, why });
export const no = (why: string): Answer => ({ yes: false, why });
export const keep = (why: string): Verdict => ({ verdict: "keep", why });
export const change = (why: string): Verdict => ({ verdict: "change", why });
export const remove = (why: string): Verdict => ({ verdict: "remove", why });

// ---- gestures: the usual answers

type Gestures = Record<Gesture, string>;

/**
 * When a press fires (owner's decision 20): what plays fires on the way down, everything else on the way up.
 *
 * | kind of control       | fires on     | why                          |
 * |-----------------------|--------------|------------------------------|
 * | play                  | the way down | instant, in the dark         |
 * | edit, setting, view   | the way up   | sliding off cancels the press |
 */
/** A button that plays. It fires on the way down. */
export const liveButton = (does: string): Gestures => ({
  press: `${does}. Fires on the way down`,
  hold: "Nothing more: it fired on the way down",
  "double press": "Two presses",
  drag: "Nothing. Sliding off does not take it back",
  keyboard: "Tab to it, then Enter or Space",
  touch: "As a press. 44 px or more",
});

/** A button of an editor. It fires on the way up, so sliding off cancels. */
export const button = (does: string): Gestures => ({
  press: `${does}. Fires on the way up`,
  hold: "Nothing more",
  "double press": "Two presses",
  drag: "Sliding off before lifting cancels the press",
  keyboard: "Tab to it, then Enter or Space",
  touch: "As a press. 44 px",
});

/** A button that stays on until pressed again. */
export const toggle = (on: string, offDoes: string): Gestures => ({
  ...button(on),
  press: `${on}. Pressed again: ${offDoes}. Fires on the way up`,
});

/** A button that plays and stays on until pressed again. It fires on the way down. */
export const liveToggle = (on: string, offDoes: string): Gestures => ({
  ...liveButton(on),
  press: `${on}. Pressed again: ${offDoes}. Fires on the way down`,
});

/** One choice out of a few, all visible: tabs, segments, pictures. */
export const choice = (does: string): Gestures => ({
  press: `${does}. Fires on the way up`,
  hold: "Nothing more",
  "double press": "As one press: the choice is already made",
  drag: "Nothing. Sliding to another option picks nothing",
  keyboard: "Tab to the group, arrows move and pick",
  touch: "As a press. 44 px",
});

/** One choice out of a few that plays: it is on the lights with the press. */
export const liveChoice = (does: string): Gestures => ({
  ...choice(does),
  press: `${does}. Fires on the way down`,
});

/** A value from 0 to 100 on a track. */
export const fader = (what: string): Gestures => ({
  press: `A press on the track jumps ${what} to that place`,
  hold: "Nothing more",
  "double press": "Two jumps to the same place",
  drag: `${what} follows the finger, also when it leaves the track`,
  keyboard: "Arrows change it by 1, with Shift by 10. Page up and down by 10, Home and End to the ends",
  touch: "As a drag. The track is 44 px wide",
});

/** A line of text. */
export const field = (what: string, saves = "Enter saves, Escape puts the old text back"): Gestures => ({
  press: "Puts the cursor in the field",
  hold: "Selects a word, as the system does",
  "double press": "Selects a word",
  drag: "Selects text",
  keyboard: `Type ${what}. ${saves}`,
  touch: "Opens the on-screen keyboard. 44 px high",
});

/** A list that opens. */
export const select = (does: string): Gestures => ({
  press: `Opens the list. A press on an entry: ${does}`,
  hold: "Nothing more",
  "double press": "Opens and closes the list",
  drag: "Nothing",
  keyboard: "Tab to it, arrows change the choice, Enter opens the list",
  touch: "Opens the system's picker. 44 px high",
});

/** Something that is pressed to pick it and dragged to move it. */
export const draggable = (pressDoes: string, dragDoes: string, keys: string): Gestures => ({
  press: `${pressDoes}`,
  hold: "Nothing more. A drag needs 8 px of movement, not a wait",
  "double press": "Two presses",
  drag: `${dragDoes}. Escape puts it back`,
  keyboard: keys,
  touch: "As press and drag. 44 px",
});

// ---- cells: the usual answers

/**
 * What the lock holds while LIVE is on (owner's decision 21): everything that changes what a press does.
 *
 * | held by the lock | its controls                                             |
 * |------------------|----------------------------------------------------------|
 * | the layout       | Edit surface, and with it every widget and the drawer    |
 * | pads             | Put on Console, which look a pad plays                   |
 * | pages            | add, rename, reorder, delete                             |
 * | Learn            | the Learn button                                         |
 * | mappings         | edit and remove a mapping. Test changes nothing: it works |
 *
 * Each is off and says this. Everything else works.
 */
export const LOCKED = off("Locked while LIVE is on");

const LEARNED = works("It is picked for Learn. The press maps it and does not fire it");
/** No control of a screen is there without the window. Only the rows of the menu bar say otherwise. */
const CLOSED = gone("The window is closed");

/** What a control of each kind does in each situation, unless its row says otherwise. */
export const USUAL: Record<Kind, Record<SituationId, Cell>> = {
  play: {
    "live-off": works("Works. The lights follow at once: there is no preview"),
    "live-on": works("Works the same"),
    lock: works("Works. The lock holds what a press does, never the playing"),
    recording: works("Works, and it is written into the Recorded row of the song that plays"),
    autopilot: works("Works. A hand beats the autopilot on the lights it touches"),
    holding: works("Works. It adds to what you hold"),
    "no-engine": off("Engine not connected"),
    empty: works("Works. No light is patched, so nothing lights up"),
    "edit-surface": works("Works the same"),
    learn: LEARNED,
    song: works("Works the same"),
    "no-song": works("Works the same"),
    "output-lost": works("Works. The lost output shows it when it is back, the others at once"),
    "window-closed": CLOSED,
  },
  edit: {
    "live-off": works("Works. It is saved by itself, and on the lights at once if it plays"),
    "live-on": works("Works. It is a live edit: on the lights at once if it plays"),
    lock: works("Works. The lock holds only what changes a press: layout, pads, pages, Learn, mappings"),
    recording: works("Works. Edits are not recorded, only what you play"),
    autopilot: works("Works. If the autopilot plays it, the change shows at once"),
    holding: works("Works. What you hold stays on top"),
    "no-engine": off("Engine not connected"),
    empty: works("Works the same"),
    "edit-surface": works("Works the same"),
    learn: LEARNED,
    song: works("Works the same"),
    "no-song": works("Works the same"),
    "output-lost": works("Works. The show file does not depend on the outputs"),
    "window-closed": CLOSED,
  },
  setting: {
    "live-off": works("Works. It takes effect at once"),
    "live-on": works("Works. It takes effect at once, mid-show"),
    lock: works("Works. The lock holds only what changes a press: layout, pads, pages, Learn, mappings"),
    recording: works("Works. Settings are not recorded"),
    autopilot: works("Works the same"),
    holding: works("Works the same"),
    "no-engine": off("Engine not connected"),
    empty: works("Works the same"),
    "edit-surface": works("Works the same"),
    learn: LEARNED,
    song: works("Works the same"),
    "no-song": works("Works the same"),
    "output-lost": works("Works the same"),
    "window-closed": CLOSED,
  },
  view: {
    "live-off": works("Works. It changes only what this screen shows"),
    "live-on": works("Works the same"),
    lock: works("Works the same"),
    recording: works("Works. It is not recorded"),
    autopilot: works("Works the same"),
    holding: works("Works the same"),
    "no-engine": works("Works: it is the screen's own state. What it shows is the last the engine sent"),
    empty: works("Works the same"),
    "edit-surface": works("Works the same"),
    learn: LEARNED,
    song: works("Works the same"),
    "no-song": works("Works the same"),
    "output-lost": works("Works the same"),
    "window-closed": CLOSED,
  },
};

const FEEDBACK: Record<Kind, Feedback> = {
  play: {
    screen: "Pressed look on the way down, before the engine answers. Its state when the engine reports it, in the same screen frame",
    controller: "The mapped control shows the same state",
  },
  edit: { screen: "The change shows when the engine reports it, in the same screen frame", controller: "None" },
  setting: { screen: "The new setting shows when the engine reports it, in the same screen frame", controller: "The mapped control shows on or off" },
  view: { screen: "At once: the screen owns this state", controller: "None" },
};

const UNDO: Record<Kind, Answer> = {
  play: no("Playing is not a change to the show file. Its opposite is at hand: stop, Release"),
  edit: yes("One step in Undo"),
  setting: yes("One step in Undo"),
  view: no("Nothing in the show file changes"),
};

/** What a row may state. Everything else comes from its kind. */
export interface Row {
  id: string;
  name: string;
  screen: Screen;
  part: string;
  from: readonly string[];
  /** Instead of `from`, for a control no mockup shows: the issue that decided it. */
  decided?: string;
  kit: Kit;
  kind: Kind;
  does: string;
  sends?: readonly CommandName[];
  ui?: string;
  gestures: Gestures;
  /** Only the situations where it differs from its kind. */
  in?: Partial<Record<SituationId, Cell>>;
  feedback?: Partial<Feedback>;
  undo?: Answer;
  /** Default: yes. Text fields, lists that open and things that are only dragged say no. */
  learn?: Answer;
  first?: Verdict;
  machine?: MachineName;
  states?: readonly string[];
}

/** A full row of the behaviour table: no cell is empty. */
export function control(row: Row): Control {
  const learn = row.learn ?? yes("A button or a value: one mapping");
  const usual = USUAL[row.kind];
  const cells = {
    ...usual,
    learn: learn.yes ? usual.learn : works(`Not picked: ${lowerFirst(learn.why)}. It works as usual and Learn keeps waiting`),
    ...row.in,
    ...(row.screen === "Menu bar" ? {} : { "window-closed": CLOSED }),
  };
  return {
    id: row.id,
    name: row.name,
    screen: row.screen,
    part: row.part,
    from: row.from,
    decided: row.decided ?? "",
    kit: row.kit,
    kind: row.kind,
    does: row.does,
    sends: row.sends ?? [],
    ui: row.ui ?? "",
    gestures: row.gestures,
    cells,
    feedback: { ...FEEDBACK[row.kind], ...row.feedback },
    undo: row.undo ?? UNDO[row.kind],
    learn,
    first: row.first ?? keep("It says what it is and what a press does"),
    ...(row.machine ? { machine: row.machine } : {}),
    states: row.states ?? [],
  };
}

const lowerFirst = (text: string) => text.charAt(0).toLowerCase() + text.slice(1);

/** Why a text field, a list or a drag handle is not learned. */
export const NOT_A_BUTTON = no("Text cannot come from a pad or a fader");
export const NOT_A_LIST = no("A list that opens has no pad or fader to stand for it");
export const ONLY_DRAGGED = no("Arranging is done on screen, not from a controller");

/** The same cell in every situation. */
export const everywhere = (cell: Cell): Record<SituationId, Cell> => ({
  "live-off": cell,
  "live-on": cell,
  lock: cell,
  recording: cell,
  autopilot: cell,
  holding: cell,
  "no-engine": cell,
  empty: cell,
  "edit-surface": cell,
  learn: cell,
  song: cell,
  "no-song": cell,
  "output-lost": cell,
  "window-closed": cell,
});
