import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { findingFaults, inventoryFaults, machineFaults, rowFaults, unusedCommands } from "./controls/check.ts";
import { CONTRADICTIONS } from "./controls/findings.ts";
import { MACHINES, walk } from "./controls/machines.ts";
import { control, button, NOT_A_BUTTON } from "./controls/spec.ts";
import { off } from "./controls/situations.ts";
import { CONTROLS, NOT_CONTROLS } from "./controls/table.ts";

const keys = JSON.parse(readFileSync(new URL("./controls/inventory.json", import.meta.url), "utf8")).map((e) => e.key);

// The table as it is: each check finds nothing.
const checks = [
  ["no row has an empty cell, an empty gesture or a reason too long for a label", () => rowFaults(CONTROLS)],
  ["every element of the mockups has a row or a reason why not", () => inventoryFaults(CONTROLS, NOT_CONTROLS, keys)],
  ["the command list is the union of what the controls send", () => unusedCommands(CONTROLS)],
  ["every state of every machine is reached, every machine is used", () => machineFaults(CONTROLS)],
  ["every contradiction points at rows that exist", () => findingFaults(CONTROLS, CONTRADICTIONS)],
];
for (const [what, faults] of checks) test(`behaviour table: ${what}`, () => assert.deepEqual(faults(), []));

const row = (over) => control({ id: "x.y", name: "Y", screen: "Console", part: "P", from: ["a"], kit: "Button.action", kind: "play", does: "Does", sends: ["TapTempo"], gestures: button("Does"), ...over });

// [what, rows, not controls, inventory keys, faults]
const faultCases = [
  ["a clean row", [row({})], {}, ["a"], []],
  ["an id used twice", [row({}), row({})], {}, ["a"], ["x.y: the id is used twice"]],
  ["a name that is not stable", [row({ id: "Pad 1" })], {}, ["a"], ["Pad 1: not a stable name (screen.part.control)"]],
  ["a row no mockup shows, decided on an issue", [row({ from: [], decided: "#107" })], {}, [], []],
  ["a row from nowhere", [row({ from: [] })], {}, [], ["x.y: stands for no element of the mockups, and names no issue that decided it"]],
  ["an empty cell", [row({ in: { lock: off("") } })], {}, ["a"], ['x.y: the cell "Lock on" is empty']],
  ["a reason too long to show", [row({ in: { lock: off("x".repeat(41)) } })], {}, ["a"], ['x.y: the reason in "Lock on" does not fit under a label (41 letters)']],
  ["neither a command nor a state of the screen", [row({ sends: [], kind: "view" })], {}, ["a"], ["x.y: sends no command and names no state of the screen"]],
  ["a control that plays and keeps a state of its own", [row({ ui: "who wins" })], {}, ["a"], ["x.y: plays or sets, and keeps a state of the screen (who wins). It sends a command, the engine decides"]],
  ["an element nobody stands for", [row({})], {}, ["a", "b"], ["b: an element of the mockups with no row and no reason"]],
  ["an element that is gone from the mockups", [row({})], { c: { is: "kit", why: "w" } }, ["a"], ["c: named in the table, not in the inventory"]],
  ["a row and not a control at once", [row({})], { a: { is: "kit", why: "w" } }, ["a"], ["a: a row (x.y) and \"not a control\" at once"]],
];
for (const [what, rows, not, inventory, expected] of faultCases) {
  test(`faults: ${what}`, () => assert.deepEqual([...rowFaults(rows), ...inventoryFaults(rows, not, inventory)], expected));
}

// [what, row, the cell with the window closed]
const closedCases = [
  ["a control of a screen is not there", row({}), { is: "gone", says: "The window is closed" }],
  ["a row cannot say otherwise", row({ in: { "window-closed": off("x") } }), { is: "gone", says: "The window is closed" }],
  ["a row of the menu bar says its own", row({ screen: "Menu bar", in: { "window-closed": off("x") } }), { is: "off", says: "x" }],
];
for (const [what, built, expected] of closedCases) {
  test(`window closed: ${what}`, () => assert.deepEqual(built.cells["window-closed"], expected));
}

test("a row that cannot be learned says so in its Learn cell", () => {
  const cell = row({ learn: NOT_A_BUTTON }).cells.learn;
  assert.deepEqual(cell, { is: "works", says: "Not picked: text cannot come from a pad or a fader. It works as usual and Learn keeps waiting" });
});

// [machine, events, the state it ends in, commands sent]
const walks = [
  ["pad", ["down", "upShort"], "on", ["StartLook"]],
  ["pad", ["down", "upLong"], "off", ["StartLook", "EndFlash"]],
  ["pad", ["down", "upShort", "down", "upShort"], "kept", ["StartLook", "StopLook"]],
  ["pad", ["down", "upShort", "down", "upLong"], "on", ["StartLook"]],
  ["pad", ["down", "upShort", "down", "upShort", "release"], "off", ["StartLook", "StopLook"]],
  ["pad", ["down", "upShort", "down", "upShort", "down", "upLong"], "off", ["StartLook", "StopLook", "StartLook", "EndFlash"]],
  ["pad", ["down", "release", "upLong"], "off", ["StartLook"]],
  ["tryPad", ["down", "upShort", "down", "upShort"], "off", ["StartLook", "EndFlash"]],
  ["blackout", ["press"], "latched", ["Blackout"]],
  ["blackout", ["press", "press"], "off", ["Blackout", "Blackout"]],
  ["blackout", ["spaceDown", "spaceUp"], "off", ["Blackout", "Blackout"]],
  ["blackout", ["spaceDown", "press", "spaceUp"], "latched", ["Blackout"]],
  ["blackout", ["press", "spaceDown", "spaceUp"], "latched", ["Blackout"]],
  ["indicator", ["liveOn"], "on", []],
  ["indicator", ["press", "liveOn"], "offLive", []],
  ["indicator", ["liveOn", "press", "press"], "on", []],
  ["indicator", ["liveOn", "press", "liveOff", "press"], "ready", []],
  ["learn", ["press", "pick", "moved"], "off", ["StartLearn"]],
  ["learn", ["press", "pick", "cancel"], "off", ["StartLearn", "CancelLearn"]],
  ["learn", ["press", "cancel"], "off", []],
  ["learn", ["press", "press"], "off", []],
  ["surface", ["edit", "lockOn"], "locked", []],
  ["surface", ["lockOn", "edit"], "locked", []],
  ["surface", ["lockOn", "lockOff", "edit", "done"], "playing", []],
  ["addLights", ["pick", "next", "next", "yes"], "find", ["Identify", "AddFixture", "Identify"]],
  ["addLights", ["pick", "next", "next", "no", "next"], "flash", ["Identify", "Identify", "Identify"]],
  ["addLights", ["pick", "next", "startOver"], "find", []],
  ["drag", ["down", "up"], "idle", []],
  ["drag", ["down", "move", "move", "up"], "idle", []],
  ["drag", ["down", "move", "cancel"], "idle", []],
];
for (const [name, events, end, sent] of walks) {
  test(`${name}: ${events.join(", ")}`, () => {
    const result = walk(MACHINES[name], events);
    assert.equal(result.states.at(-1), end);
    assert.deepEqual(result.sent, sent);
  });
}
