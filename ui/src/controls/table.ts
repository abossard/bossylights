// The behaviour table: every control of the eight mockups and of the menu bar, one row each (issue #102).
// And every element of the mockups that is not a control of the product, with the reason.
import type { Control } from "./spec.ts";
import { CONSOLE } from "./rows/console.ts";
import { DMX } from "./rows/dmx.ts";
import { EXPLAINERS } from "./rows/explainers.ts";
import { INOUT } from "./rows/inout.ts";
import { LOOKS } from "./rows/looks.ts";
import { MENUBAR } from "./rows/menubar.ts";
import { PATCH } from "./rows/patch.ts";
import { SONGS } from "./rows/songs.ts";
import { TOP } from "./rows/top.ts";

export const CONTROLS: readonly Control[] = [...TOP, ...CONSOLE, ...SONGS, ...LOOKS, ...PATCH, ...DMX, ...INOUT, ...EXPLAINERS, ...MENUBAR];

/**
 * | is      | means                                                                            |
 * |---------|----------------------------------------------------------------------------------|
 * | kit     | the inside of a component: counted where the component is used                   |
 * | wiring  | a component that only hands callbacks on: its controls are counted one by one    |
 * | tooling | part of the mockups, not of the product: the review layer, the simulated hardware |
 */
export interface NotAControl {
  is: "kit" | "wiring" | "tooling";
  why: string;
}

const kit = (why: string): NotAControl => ({ is: "kit", why });
const wiring = (why: string): NotAControl => ({ is: "wiring", why });
const tooling = (why: string): NotAControl => ({ is: "tooling", why });

const REVIEW = tooling("The review layer of the mockups. The catalog has its own rating and comment");

/** Elements of the inventory that are not rows of the table. Keys of `inventory.json`. */
export const NOT_CONTROLS: Record<string, NotAControl> = {
  "kit/Button/button": kit("The inside of Button"),
  "kit/Fader/div": kit("The inside of Fader"),
  "kit/List/button": kit("The inside of List: a row that can be picked"),
  "kit/Pad/running": kit("The inside of Pad"),
  "kit/RigView/g": kit("The map of the lights. No screen uses it since review 1: no map anywhere"),
  "kit/Segmented/div": kit("The inside of Segmented: its arrow keys"),
  "kit/Segmented/button": kit("The inside of Segmented: one option"),
  "kit/Table/button": kit("The inside of Table: a row that can be picked"),
  "kit/Tabs/div": kit("The inside of Tabs: its arrow keys"),
  "kit/Tabs/button": kit("The inside of Tabs: one tab"),
  "kit/XYPad/div": kit("The inside of XYPad"),
  "kit/Region/note-on": REVIEW,
  "looks/Screen/library": wiring("The library as a whole. Its controls are rows of their own"),
  "looks/Screen/sequenceeditor": wiring("The sequence editor as a whole"),
  "looks/Screen/connections": wiring("The Connections part as a whole"),
  "patch/Screen/addflow": wiring("The Add lights flow as a whole"),
  "patch/Screen/panelsetup": wiring("The tools of a panel as a whole: 'Change panel' opens the form"),
  "looks/StepEditor/othercolour": wiring("The colour wheel as a whole: 'Add this colour' hands the colour on"),
  "review/ReviewPanel/close": REVIEW,
  "review/ReviewPanel/a": REVIEW,
  "review/ReviewPanel/button": REVIEW,
  "review/ReviewPanel/of-5": REVIEW,
  "review/ReviewPanel/what-works-what-does-not": REVIEW,
  "review/ReviewPanel/input-checkbox": tooling("A switch of the review panel that fakes 'no lights connected'"),
  "review/ReviewPanel/input-checkbox#2": tooling("A switch of the review panel that empties deck 2"),
  "review/ReviewPanel/comment-mode": REVIEW,
  "review/ReviewPanel/no-note-yet": REVIEW,
  "review/ReviewPanel/what-should-change-in-this": REVIEW,
  "review/ReviewPanel/back-to-regions": REVIEW,
  "review/ReviewScreen/copy-as-markdown": REVIEW,
  "review/ReviewScreen/export": REVIEW,
  "review/ReviewScreen/table": REVIEW,
  "shell/TopBar/review-this-screen": REVIEW,
  "shell/TopBar/done": REVIEW,
  "shell/LearnBar/button": tooling("Simulated hardware: in the product a real controller is moved"),
  "modules/Screen/the-author-s-lean": tooling("A review question with two answers. Questions go to the decisions page now"),
  "modules/Screen/your-words-on": tooling("The text field of a review question"),
};
