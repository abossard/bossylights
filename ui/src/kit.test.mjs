import assert from "node:assert/strict";
import { test } from "node:test";
import { keyStep, lampOf, level, neighbour, scaled, placeIn, unbroken, wheelAt, wheelColour, wheelSpot } from "./kit/common.ts";
import { dragOutcome } from "./kit/dragging.ts";
import { fieldStep, judge } from "./kit/editing.ts";

// [value, the level it becomes]
const levels = [[-5, 0], [0, 0], [49.5, 50], [100, 100], [140, 100]];
for (const [value, expected] of levels) test(`level: ${value} is ${expected}`, () => assert.equal(level(value), expected));

// [value, the top of the range, the value it becomes]. A DMX channel holds 0 to 255 (DMX512), a level 0 to 100.
const ranges = [[-1, 255, 0], [0, 255, 0], [127.6, 255, 128], [255, 255, 255], [300, 255, 255], [140, 100, 100]];
for (const [value, most, expected] of ranges) test(`scaled: ${value} of 0 to ${most} is ${expected}`, () => assert.equal(scaled(value, most), expected));

const BOX = { left: 100, top: 50, width: 200, height: 100 };
// [what, x, y, across, up]
const places = [
  ["the bottom left corner", 100, 150, 0, 0],
  ["the top right corner", 300, 50, 100, 100],
  ["the middle", 200, 100, 50, 50],
  ["outside, left and below", 0, 400, 0, 0],
  ["outside, right and above", 900, -40, 100, 100],
];
for (const [what, x, y, across, up] of places) test(`placeIn: ${what}`, () => assert.deepEqual(placeIn(BOX, x, y), { across, up }));

const ROUND = { left: 100, top: 100, width: 200, height: 200 };
// [what, x, y, around, out]
const turns = [
  ["the top of the rim", 200, 100, 0, 100],
  ["the right of the rim", 300, 200, 25, 100],
  ["the bottom of the rim", 200, 300, 50, 100],
  ["the left of the rim", 100, 200, 75, 100],
  ["the middle", 200, 200, 0, 0],
  ["half way out to the right", 250, 200, 25, 50],
  ["outside the wheel, below", 200, 900, 50, 100],
  ["just left of the top: the hue does not jump to 100", 199, 100, 0, 100],
];
for (const [what, x, y, around, out] of turns) test(`wheelAt: ${what}`, () => assert.deepEqual(wheelAt(ROUND, x, y), { around, out }));
test("wheelAt: a wheel with no size is at 0", () => assert.deepEqual(wheelAt({ left: 0, top: 0, width: 0, height: 0 }, 5, 5), { around: 0, out: 0 }));

// [around, out, left, top, colour]
const spots = [
  [0, 100, 50, 0, "hsl(0 100% 50%)"],
  [25, 100, 100, 50, "hsl(90 100% 50%)"],
  [50, 50, 50, 75, "hsl(180 100% 75%)"],
  [75, 0, 50, 50, "hsl(270 100% 100%)"],
];
for (const [around, out, left, top, colour] of spots) {
  test(`a place on the wheel: ${around} around, ${out} out`, () => {
    assert.deepEqual(wheelSpot(around, out), { left, top });
    assert.equal(wheelColour(around, out), colour);
  });
}

test("placeIn: a box with no size is at 0", () => assert.deepEqual(placeIn({ left: 0, top: 0, width: 0, height: 0 }, 5, 5), { across: 0, up: 0 }));

// [key, shift, step]
const keys = [
  ["ArrowUp", false, 1],
  ["ArrowRight", false, 1],
  ["ArrowDown", false, -1],
  ["ArrowLeft", true, -10],
  ["ArrowUp", true, 10],
  ["PageUp", false, 10],
  ["PageDown", false, -10],
  ["Home", false, "start"],
  ["End", false, "end"],
  ["a", false, undefined],
];
for (const [key, shift, expected] of keys) test(`keyStep: ${key}${shift ? " with Shift" : ""}`, () => assert.equal(keyStep(key, shift), expected));

const OPTIONS = ["half", "normal", "double"];
// [selected, key, the option it moves to]
const moves = [
  ["half", "ArrowRight", "normal"],
  ["double", "ArrowRight", "half"],
  ["half", "ArrowLeft", "double"],
  ["normal", "ArrowDown", "double"],
  ["normal", "ArrowUp", "half"],
  ["normal", "Enter", undefined],
  ["gone", "ArrowRight", "normal"],
];
for (const [selected, key, expected] of moves) test(`neighbour: ${key} from ${selected}`, () => assert.equal(neighbour(OPTIONS, selected, key), expected));
test("neighbour: no options, no move", () => assert.equal(neighbour([], "x", "ArrowRight"), undefined));

test("unbroken: a hyphen that does not break", () => assert.equal(unbroken("Chase L-R"), "Chase L‑R"));

// [toggle state, the lamp's shape, the word under the name, what a screen reader hears after the name]
const lamps = [
  ["off", "dashed", "", "Off"],
  ["ready", "outline", "Ready", "Ready, starts with LIVE"],
  ["on", "solid", "On", "On"],
];
for (const [state, fill, word, says] of lamps) {
  test(`lampOf: ${state}`, () => {
    assert.deepEqual(lampOf(state), { fill, word, says });
    assert.ok(says.startsWith(word), "the name starts with the word it shows");
  });
  test(`lampOf: a caller cannot change the next ${state} lamp`, () => {
    assert.equal(Reflect.set(lampOf(state), "fill", "changed"), false);
    assert.equal(lampOf(state).fill, fill);
  });
}

// [from, event, to, what it means]
const outcomes = [
  ["idle", "down", "armed", "none"],
  ["armed", "up", "idle", "press"],
  ["armed", "move", "dragging", "move"],
  ["armed", "cancel", "idle", "cancel"],
  ["dragging", "move", "dragging", "move"],
  ["dragging", "up", "idle", "drop"],
  ["dragging", "cancel", "idle", "cancel"],
  ["idle", "up", "idle", "none"],
  ["idle", "cancel", "idle", "none"],
];
for (const [from, event, to, expected] of outcomes) test(`drag: ${event} in ${from}`, () => assert.equal(dragOutcome(from, to, event), expected));

// A text field, as in docs/design/ui-states.md and Field's own comment: Enter saves, Escape puts the old value back,
// an empty or unchanged text keeps the old value, a refused text says why, leaving refuses or marks unsaved.
// [state, event, state after, effect]
const SAME = { is: "same" };
const NO = { is: "refused", reason: "Too long" };
const FITS = { is: "fits", text: "Wash" };
const fieldStates = [{ is: "clean" }, { is: "refused", reason: "Taken" }, { is: "unsaved" }];
const fieldCases = [
  [{ is: "typed" }, { is: "clean" }, { is: "none" }],
  [{ is: "enter", text: SAME }, { is: "clean" }, { is: "putBack" }],
  [{ is: "enter", text: NO }, NO, { is: "none" }],
  [{ is: "enter", text: FITS }, { is: "clean" }, { is: "send", text: "Wash" }],
  [{ is: "escape" }, { is: "clean" }, { is: "putBack" }],
  [{ is: "leave", text: SAME, saves: true }, { is: "clean" }, { is: "putBack" }],
  [{ is: "leave", text: NO, saves: true }, NO, { is: "putBack" }],
  [{ is: "leave", text: FITS, saves: true }, { is: "clean" }, { is: "send", text: "Wash" }],
  [{ is: "leave", text: SAME, saves: false }, { is: "clean" }, { is: "putBack" }],
  [{ is: "leave", text: NO, saves: false }, NO, { is: "putBack" }],
  [{ is: "leave", text: FITS, saves: false }, { is: "unsaved" }, { is: "none" }],
];
for (const state of fieldStates) {
  for (const [event, after, effect] of fieldCases) {
    test(`field ${JSON.stringify(state)} + ${JSON.stringify(event)}`, () => assert.deepEqual(fieldStep(state, event), [after, effect]));
  }
}

// [typed, old value, refuses, judged]
const refuseLong = (text) => (text.length > 4 ? "Too long" : undefined);
const judgements = [
  ["", "Wash", refuseLong, SAME],
  ["  Wash ", "Wash", refuseLong, SAME],
  ["Beam", "Wash", refuseLong, { is: "fits", text: "Beam" }],
  [" Beam ", "Wash", undefined, { is: "fits", text: "Beam" }],
  ["Spotlight", "Wash", refuseLong, NO],
];
for (const [typed, value, refuse, expected] of judgements) {
  test(`judge ${JSON.stringify(typed)} over ${value}`, () => assert.deepEqual(judge(typed, value, refuse), expected));
}
