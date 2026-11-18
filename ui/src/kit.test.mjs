import assert from "node:assert/strict";
import { test } from "node:test";
import { keyStep, level, neighbour, placeIn, unbroken } from "./kit/common.ts";
import { dragOutcome } from "./kit/dragging.ts";

// [value, the level it becomes]
const levels = [[-5, 0], [0, 0], [49.5, 50], [100, 100], [140, 100]];
for (const [value, expected] of levels) test(`level: ${value} is ${expected}`, () => assert.equal(level(value), expected));

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
