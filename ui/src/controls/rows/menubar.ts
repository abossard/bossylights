// The menu bar item: what is there while the engine runs, with or without the window.
// No mockup shows it. Decided on #107: engine state, frame counter, Open window, Stop engine.
import { off, works } from "../situations.ts";
import { control, no, type Control, type Row } from "../spec.ts";

const OUTSIDE = no("It is outside the window, drawn by the system");
const LIVE_IS_ON = off("LIVE is on");

/** An entry of a menu of the system. It fires on the way up, as every menu does. */
const entry = (does: string) => ({
  press: `${does}. Fires on the way up, then the menu closes`,
  hold: "Nothing more",
  "double press": "As one press: the menu is closed after the first",
  drag: "Press the item, slide to the entry, lift: the same as a press",
  keyboard: "Arrows move through the open menu, Enter picks, Escape closes it",
  touch: "As a press. The system sets the size",
});

const menu = (row: Omit<Row, "screen" | "part" | "from" | "decided" | "kit">): Control =>
  control({ learn: OUTSIDE, ...row, screen: "Menu bar", part: "Menu bar item", from: [], decided: "#107", kit: "Item.entry" });

export const MENUBAR: Control[] = [
  menu({
    id: "menubar.item",
    name: "Menu bar item",
    kind: "view",
    does: "Says that the engine runs, also with no window. A press opens its menu: the state in words, the frame counter, Open window, Stop engine",
    ui: "its menu, open or closed",
    gestures: {
      press: "Opens the menu. Fires on the way down, as every menu of the system",
      hold: "Nothing more",
      "double press": "Opens and closes the menu",
      drag: "Slide to an entry and lift: picks it",
      keyboard: "The system's way into the menu bar, then arrows",
      touch: "As a press. The system sets the size",
    },
    in: {
      "live-off": works("Works. The lamp is mute: the engine runs, LIVE is off"),
      "live-on": works("Works. The lamp is paper and the menu says 'LIVE on'"),
      lock: works("Works. Nothing locks the menu bar item"),
      "no-engine": works("Works. The lamp is empty and the menu says 'Engine not connected'"),
      empty: works("Works. The lamp is red and the menu says 'No lights connected'"),
      learn: works("Not picked: it is outside the window. Learn keeps waiting"),
      "output-lost": works("Works. The lamp is red and the menu names the lost output"),
      "window-closed": works("Works. It is the only sign that the engine runs"),
    },
    feedback: {
      screen: "The lamp follows the engine's state when it is reported. The frame counter counts only inside the open menu: a number that runs in the menu bar is noise",
      controller: "None",
    },
    states: ["running", "LIVE on", "trouble", "not connected", "menu open"],
  }),
  menu({
    id: "menubar.open-window",
    name: "Open window",
    kind: "view",
    does: "Opens the window, or brings it to the front. The lights do not notice",
    ui: "the window, open or closed. It is the shell's own state",
    gestures: entry("Opens the window"),
    in: {
      "live-off": works("Brings the window to the front"),
      "live-on": works("Brings the window to the front. The show runs on"),
      "no-engine": works("Brings the window to the front. Its trouble chip says 'Engine not connected'"),
      learn: works("Not picked: it is outside the window. Learn keeps waiting"),
      "window-closed": works("Opens the window. It shows the engine as it is now, mid-show too"),
    },
    feedback: { screen: "The window is in front, at once", controller: "None" },
  }),
  menu({
    id: "menubar.stop-engine",
    name: "Stop engine",
    kind: "setting",
    does: "Ends the engine in an orderly way. No frame is sent after it: this is the one control that ends the lights",
    sends: ["StopEngine"],
    gestures: entry("Stops the engine"),
    in: {
      "live-off": works("Stops the engine. The window, if open, closes with it and the item goes"),
      "live-on": LIVE_IS_ON,
      lock: LIVE_IS_ON,
      recording: LIVE_IS_ON,
      autopilot: works("Works. The autopilot ends with the engine"),
      holding: works("Works. What you hold ends with the engine"),
      "no-engine": off("Engine not connected"),
      learn: works("Not picked: it is outside the window. Learn keeps waiting"),
      "output-lost": works("Works the same"),
      "window-closed": works("Stops the engine. The item goes: nothing of the app runs any more"),
    },
    feedback: { screen: "The item leaves the menu bar when the engine has ended", controller: "None" },
    undo: no("Start the app again. The show file was saved by itself"),
    states: ["works", "off: LIVE is on"],
  }),
];
