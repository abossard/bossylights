// What the pass over every control found: contradictions and how each is resolved,
// and the controls no mockup shows (issue #102). Removals and changes are on the rows themselves.

/**
 * | kind      | means                                              |
 * |-----------|----------------------------------------------------|
 * | two-one   | two controls for one thing                         |
 * | one-two   | one control, or one word, meaning two things       |
 * | no-reason | a control that is off with no reason shown         |
 * | rule      | a control that works against a rule of the product |
 */
export interface Contradiction {
  kind: "two-one" | "one-two" | "no-reason" | "rule";
  what: string;
  /** Ids of the rows it is about. */
  where: readonly string[];
  resolved: string;
  /** The number of the question on the decisions page, when the owner has to say. */
  asks?: number;
}

export const CONTRADICTIONS: readonly Contradiction[] = [
  // ---- two controls for one thing
  { kind: "two-one", what: "Tap in the top bar and Tap on In/Out", where: ["top.tap", "inout.clock.tap"], resolved: "The one on In/Out goes. The top bar is on every screen" },
  { kind: "two-one", what: "The breadcrumb and 'Back to' in the Looks editor", where: ["looks.crumb", "looks.back"], resolved: "'Back to' goes. The breadcrumb says where it leads" },
  { kind: "two-one", what: "The universe tabs and the rows of the Output table on DMX", where: ["dmx.universe", "dmx.output.row"], resolved: "The rows stop being buttons. A status table shows status" },
  { kind: "two-one", what: "The order grip and the two arrows on a light in Patch", where: ["patch.fixtures.order", "patch.fixtures.move"], resolved: "The grip goes. The arrows work by touch and by keyboard" },
  { kind: "two-one", what: "'Edit surface' in the page bar and again on an empty page", where: ["console.edit-surface"], resolved: "One button, in the page bar. It is paper while the page is empty" },
  { kind: "two-one", what: "Two Cancel buttons in the Learn bar, one per step, with different looks", where: ["top.learn.cancel"], resolved: "One Cancel, the same in both steps" },
  { kind: "two-one", what: "The Autopilot indicator at the LIVE switch and the Autopilot button in Running", where: ["top.indicator.autopilot", "console.autopilot.on"], resolved: "Kept: one state and one command, shown in two places. They always agree" },
  { kind: "two-one", what: "The style list on the Console and 'Use this style' on Looks", where: ["console.autopilot.style", "looks.autopilot.use-style"], resolved: "Kept: one command from two places, each where you are when you want it" },
  { kind: "two-one", what: "A section can be picked in the Sections row and in the Show row", where: ["songs.section"], resolved: "Kept as one control in two rows: the same section, the same selection" },
  { kind: "two-one", what: "Three controls say what is followed: the DJ link indicator, 'Follows' on Songs, the clock feed on In/Out", where: ["top.indicator.dj", "songs.follows", "inout.clock.feed"], resolved: "DJ link is on or off. 'Follows' picks the deck and moves to In/Out, beside the clock feed that picks the beat", asks: 26 },

  // ---- one control or one word, two meanings
  { kind: "one-two", what: "The '+' in the Console pages is a tab that is an action", where: ["console.pages.add"], resolved: "A button after the tabs, there only while editing" },
  { kind: "one-two", what: "The cross takes a thing out of a place (a look out of a group, a colour out of a step). On a group header it deletes the group", where: ["looks.library.group.delete", "looks.groups.remove", "songs.section.remove-look", "looks.step.colour.remove"], resolved: "The cross always means 'take out of this place'. Deleting a group says 'Delete group'", asks: 27 },
  { kind: "one-two", what: "'Release all' on the Console gives back everything. 'Release all' on DMX gives back raw channels only", where: ["console.running.release-all", "dmx.held.release-all"], resolved: "The Console one gives back raw channels too. The DMX one says 'Release channels'", asks: 22 },
  { kind: "one-two", what: "'Play from', 'Stop listening' and the flag 'Listening' name one thing, and no music plays", where: ["songs.play-from"], resolved: "One word, the one Looks uses for the same idea: 'Try from Drop', 'Stop'", asks: 28 },
  { kind: "one-two", what: "'In use' is a pressed button that does nothing when pressed", where: ["looks.autopilot.use-style"], resolved: "The button is gone while the style is in use. The style list says 'In use'" },
  { kind: "one-two", what: "'Undo' in the top bar and 'Undo last pass' on Songs take back different things", where: ["top.undo", "songs.recorded.undo-last-pass"], resolved: "Kept. The top bar Undo names its step and never touches the Recorded row" },
  { kind: "one-two", what: "Blackout latches from the button and holds from Space", where: ["top.blackout"], resolved: "Kept, and written down as its state machine. On touch there is no hold", asks: 30 },
  { kind: "one-two", what: "A pad fires on the way down, an editor button on the way up", where: ["console.pad", "top.blackout", "top.tap"], resolved: "A rule: what is pressed during a show fires on the way down. Editor buttons fire on the way up, so sliding off cancels", asks: 20 },

  // ---- off with no reason shown
  {
    kind: "no-reason",
    what: "Thirteen controls go off and do not say why",
    where: [
      "console.running.release-all",
      "top.undo",
      "console.cue.back",
      "songs.recorded.mute",
      "songs.recorded.undo-last-pass",
      "songs.recorded.clear",
      "songs.looks.add",
      "looks.step.colour.add",
      "looks.seq.width",
      "looks.pattern.measure",
      "patch.fixtures.move",
      "patch.details.address",
      "patch.add.back",
    ],
    resolved: "Each says its reason under its label, in a few words. Where the reason would be noise (the first step has no Back) the control is gone instead",
  },
  { kind: "no-reason", what: "Pads are off while the surface is edited, and look like pads that are broken", where: ["console.pad", "console.fader", "console.xy"], resolved: "The edit frame covers the whole widget and says its name and size: that is the reason" },
  { kind: "no-reason", what: "'Move to no free' is a button that does nothing", where: ["patch.details.move-to-free"], resolved: "Off, with 'No free place in this universe'" },

  // ---- against a rule
  { kind: "rule", what: "The lock holds 'Edit surface', but 'Add page', 'Put on Console', Learn and the mappings still change the surface while it is on", where: ["console.pages.add", "looks.put-on-console", "top.learn", "inout.mapping.edit", "inout.mapping.remove"], resolved: "The lock holds everything that changes what a press does: layout, pads, Learn, mappings", asks: 21 },
  { kind: "rule", what: "'Test' on an output puts the whole room at half, also while LIVE is on", where: ["inout.out.test"], resolved: "Off while LIVE is on, and says so" },
  { kind: "rule", what: "A clock feed with no signal can be picked: then nothing drives the beat", where: ["inout.clock.feed"], resolved: "Off, with 'No signal'" },
  { kind: "rule", what: "One press of Tap takes the beat away from the DJ software, and the way back is on In/Out", where: ["top.tap", "inout.clock.feed"], resolved: "The tempo says where it comes from. Taps hold until the next song starts, then the picked feed is back", asks: 23 },
  { kind: "rule", what: "An output that is lost, or an engine that is not connected, has no mark in the top bar", where: ["top.trouble"], resolved: "The chip that says 'No lights connected' says these two as well" },
  { kind: "rule", what: "The pads of 'How it works' play the real lights", where: ["how.pad", "how.master", "how.release", "how.record", "how.replay"], resolved: "An explainer plays on its own small model", asks: 25 },
  { kind: "rule", what: "The speed dial changes its own label and nothing else", where: ["console.speed"], resolved: "It sends SetSpeed" },
  { kind: "rule", what: "'Every action can be undone' is a principle. A pad press, a release and a blackout are not steps of Undo", where: ["top.undo", "console.pad"], resolved: "Undo takes back changes to the project. What is played is taken back by its own opposite: stop, Release", asks: 31 },
  { kind: "rule", what: "'Surprise me' is not in the Console brief, and what it starts stays held", where: ["console.surprise"], resolved: "It goes. The autopilot is the surprise", asks: 24 },
];

/** A thing the product needs a control for, and no mockup shows one. */
export interface Missing {
  screen: string;
  what: string;
  /** A command of the engine that already waits for it, if any. */
  command?: string;
}

export const MISSING: readonly Missing[] = [
  { screen: "Top bar", what: "Redo" },
  { screen: "Top bar", what: "The show file: new, open, save as, and the name of the show" },
  { screen: "Looks", what: "Make a new step, sequence or pattern" },
  { screen: "Looks", what: "Rename, copy and delete a look" },
  { screen: "Looks", what: "Add an item to a sequence, and take one out" },
  { screen: "Looks", what: "A colour outside the twelve swatches (the brief says 'chips and picker')" },
  { screen: "Console", what: "Rename, reorder and delete a page" },
  { screen: "Console", what: "Say which look a pad plays, and which fader a slider moves" },
  { screen: "Console", what: "The words of a label and a frame" },
  { screen: "Patch", what: "Remove a light, rename it, change its mode or its group" },
  { screen: "Patch", what: "Change or remove a pixel panel" },
  { screen: "In/Out", what: "Add, set up and remove an output", command: "SetOutput, RemoveOutput" },
  { screen: "In/Out", what: "Pick the audio input, and set up OSC" },
  { screen: "Console", what: "The cue list itself: its looks and their order (#70)" },
  { screen: "Outside the window", what: "The menu bar item while the window is closed: engine state, frame counter, Open window, Stop engine. Decided on #107, drawn in the catalog" },
];
