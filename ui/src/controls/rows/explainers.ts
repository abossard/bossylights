// The two explainer screens: How it works and Modules. They ship with the product (decision 25).
// They teach on a small model of their own and never touch the lights: every row is of kind view.
import { control, button, change, choice, fader, no, toggle, type Control, type Row, type Screen } from "../spec.ts";

const OWN_MODEL = change("In the mockup it plays on the same lights as the Console. The owner's decision 25: the explainers ship, and play on their own small model. They never touch the lights");
const EXPLAINS = no("An explainer is read, not played");

const explains = (screen: Screen, row: Omit<Row, "screen" | "kind" | "ui">): Control =>
  control({ learn: EXPLAINS, first: OWN_MODEL, ...row, screen, kind: "view", ui: "the explainer's own model" });

export const EXPLAINERS: Control[] = [
  explains("How it works", {
    id: "how.pad",
    name: "Pad to try",
    part: "Try it",
    from: ["how/Screen/pad"],
    kit: "Pad",
    does: "Starts or flashes a look in the explainer, so the command can be watched travelling to the lights",
    gestures: {
      press: "Starts the look in the explainer, on the way down, as a real pad does. On a running pad: it stops",
      hold: "Down for more than 350 ms it was a flash: the look ends when the finger lifts",
      "double press": "Two presses: on, then off",
      drag: "Nothing. Lifting anywhere ends the press",
      keyboard: "Tab to it, Enter is a short press",
      touch: "As a press. 44 px or more",
    },
    states: ["idle", "running", "flash"],
  }),
  explains("How it works", {
    id: "how.master",
    name: "Master to try",
    part: "Try it",
    from: ["how/Screen/master"],
    kit: "Fader",
    does: "Moves the master of the explainer",
    gestures: fader("the explainer's master"),
  }),
  explains("How it works", {
    id: "how.record",
    name: "Record",
    part: "Try it",
    from: ["how/Screen/stop-recording"],
    kit: "Button.toggle",
    does: "Writes the presses in the explainer down as commands, to show what a recording is",
    gestures: toggle("Starts writing presses down", "it stops"),
    states: ["Record", "Stop recording"],
  }),
  explains("How it works", {
    id: "how.replay",
    name: "Replay on Show",
    part: "Try it",
    from: ["how/Screen/stop-replay"],
    kit: "Button.toggle",
    does: "Sends the written commands again, on the Show row of the explainer",
    gestures: toggle("Replays what was written", "the replay stops"),
    states: ["Replay on Show", "Stop replay"],
  }),
  explains("How it works", {
    id: "how.release",
    name: "Release",
    part: "Try it",
    from: ["how/Screen/release"],
    kit: "Button.action",
    does: "Gives back what the explainer's pads hold, so what was underneath is seen to return",
    gestures: button("Releases in the explainer"),
  }),
  explains("How it works", {
    id: "how.walk",
    name: "Pick a walk",
    part: "Three walks",
    from: ["how/Screen/walk"],
    kit: "Picker.segments",
    does: "Picks one of three walks: press a pad, record and replay, the autopilot yields",
    gestures: choice("Opens that walk at its first step"),
    first: { verdict: "keep", why: "It changes only which walk shows" },
  }),
  explains("How it works", {
    id: "how.walk.step",
    name: "Walk: Back, Next, Again",
    part: "Three walks",
    from: ["how/Screen/back", "how/Screen/again", "how/Screen/start"],
    kit: "Button.action",
    does: "Steps through the walk: 'Start', then 'Next', and 'Again' at the end. 'Back' goes one step back",
    gestures: button("Goes one step on, or back"),
    first: { verdict: "keep", why: "It changes only which step shows" },
    states: ["Start", "Next", "Again"],
  }),
  explains("Modules", {
    id: "modules.node",
    name: "Module on the map",
    part: "Boundary map",
    from: ["modules/Node/g"],
    kit: "Item.tile",
    does: "Opens the card of that module: its whole interface, what it hides, which screens use it",
    gestures: { ...button("Opens that module's card"), keyboard: "Tab to it, then Enter or Space" },
    first: { verdict: "keep", why: "It changes only which card shows" },
    states: ["plain", "open"],
  }),
  explains("Modules", {
    id: "modules.trace",
    name: "Trace a press",
    part: "Boundary map",
    from: ["modules/Screen/trace-a-press"],
    kit: "Button.action",
    does: "Plays one press through the modules, from the controller to the output, as an animation",
    gestures: button("Plays the trace"),
    first: { verdict: "keep", why: "It plays an animation and nothing else" },
  }),
  explains("Modules", {
    id: "modules.jump",
    name: "Screen that uses a module",
    part: "Module card",
    from: ["modules/Screen/button"],
    kit: "Button.action",
    does: "Opens the screen the card names",
    gestures: button("Opens that screen"),
    first: { verdict: "keep", why: "A jump, like the boxes in Connections" },
  }),
];
