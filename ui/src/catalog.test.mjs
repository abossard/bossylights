import assert from "node:assert/strict";
import { test } from "node:test";
import { act } from "./catalog/act.ts";
import { byFunction, linesOf, recipe } from "./catalog/compose.ts";
import { framesOf, screensOf, search } from "./catalog/frames.ts";
import { inSituation, inState, usual } from "./catalog/look.ts";
import { asMarkdown, comment, kitKey, merge, parse, rate, reviewOf, same } from "./catalog/review.ts";
import { fileText } from "./catalog/reviews-file.mjs";
import { SAMPLES, SITUATION_STATE, STATE_LOOKS } from "./catalog/samples.ts";
import { USES } from "./catalog/uses.ts";
import { MACHINES } from "./controls/machines.ts";
import { SITUATIONS } from "./controls/situations.ts";
import { kitNames, PRIMITIVES } from "./controls/spec.ts";
import { CONTROLS } from "./controls/table.ts";

const byId = (id) => CONTROLS.find((c) => c.id === id);
const frame = (id, key) => framesOf(byId(id)).find((f) => f.key === key);

// Every gesture a frame can make, as scripts. Each frame plays all of them from its start.
const SCRIPTS = [
  [{ is: "press" }, { is: "press" }],
  [{ is: "down" }, { is: "up", long: false }, { is: "down" }, { is: "up", long: false }, { is: "release" }],
  [{ is: "down" }, { is: "up", long: true }],
  [{ is: "key", down: true }, { is: "key", down: false }],
  [{ is: "set", value: 10, value2: 20 }],
  [{ is: "text", text: "x" }],
  [{ is: "step", by: -1 }, { is: "step", by: 1 }],
  [{ is: "lift" }, { is: "drop" }],
  [{ is: "lift" }, { is: "cancel" }],
  [{ is: "remove" }],
];
const play = (control, look, script) => script.reduce((at, gesture) => {
  const result = act(control, at.look, gesture);
  return { look: result.look, sent: [...at.sent, ...result.sent] };
}, { look, sent: [] });
/** The names of every command a control sends from any frame with any gesture. */
function fired(control) {
  const names = new Set();
  for (const f of framesOf(control)) {
    for (const script of [...SCRIPTS, ...f.look.options.map((option) => [{ is: "pick", option }])]) {
      for (const sent of play(control, f.look, script).sent) names.add(sent.name);
    }
  }
  return names;
}
const machineSends = (control) => (control.machine ? Object.values(MACHINES[control.machine].table).flatMap((row) => Object.values(row).flatMap((s) => s.sends ?? [])) : []);

test("every control has a frame alone, one per situation and one per state", () => {
  for (const c of CONTROLS) assert.equal(framesOf(c).length, 1 + SITUATIONS.length + c.states.length, c.id);
});

test("no two states of a control look the same", () => {
  const same = CONTROLS.flatMap((c) => {
    const looks = c.states.map((state) => JSON.stringify({ ...inState(c, state), at: "" }));
    return c.states.filter((_, at) => looks.indexOf(looks[at]) !== at).map((state) => `${c.id}: ${state}`);
  });
  assert.deepEqual(same, []);
});

test("a frame is off or gone exactly where the row says so, with the row's words", () => {
  for (const c of CONTROLS) {
    for (const s of SITUATIONS) {
      const cell = c.cells[s.id];
      const look = inSituation(c, s.id);
      assert.equal(look.gone, cell.is === "gone" ? cell.says : "", `${c.id} in ${s.id}: gone`);
      assert.equal(look.off, cell.is === "off" ? cell.says : "", `${c.id} in ${s.id}: off`);
    }
  }
});

test("a control that is off or gone sends nothing, whatever is done to it", () => {
  const sent = CONTROLS.flatMap((c) =>
    framesOf(c)
      .filter((f) => f.is !== "works")
      .flatMap((f) => SCRIPTS.flatMap((script) => play(c, f.look, script).sent.map((s) => `${c.id} in ${f.key}: ${s.name}`))),
  );
  assert.deepEqual(sent, []);
});

test("no frame sends a command its row or its machine does not name", () => {
  const extra = CONTROLS.flatMap((c) => {
    const allowed = new Set([...c.sends, ...machineSends(c), "StartLearn"]);
    return [...fired(c)].filter((name) => !allowed.has(name)).map((name) => `${c.id}: ${name}`);
  });
  assert.deepEqual(extra, []);
});

// Commands a row names that no frame of its page can fire, each with the reason.
// [control, command, why]
const SILENT = [["console.fader", "SetGroupLevel", "the frames show the master. A group fader is the same gesture with another name"]];
test("every command a row names fires in a frame of its page, but the listed ones", () => {
  const silent = CONTROLS.flatMap((c) => c.sends.filter((name) => !fired(c).has(name)).map((name) => `${c.id}: ${name}`));
  assert.deepEqual(silent, SILENT.map(([id, name]) => `${id}: ${name}`));
});

// [what, control, frame, gestures, commands sent, what the look must hold after]
const plays = [
  ["a short press latches the pad", "console.pad", "alone", [{ is: "down" }, { is: "up", long: false }], ["StartLook"], { variant: "manual", at: "on" }],
  ["a long hold was a flash", "console.pad", "alone", [{ is: "down" }, { is: "up", long: true }], ["StartLook", "EndFlash"], { variant: "idle", at: "off" }],
  ["a press on a running pad: off, still held", "console.pad", "state:running by hand", [{ is: "down" }, { is: "up", long: false }], ["StopLook"], { variant: "kept", at: "kept" }],
  ["Release gives a kept pad back", "console.pad", "state:pad off, still held", [{ is: "release" }], [], { variant: "idle", at: "off" }],
  ["Blackout: a short press latches", "top.blackout", "alone", [{ is: "down" }, { is: "up", long: false }], ["Blackout on: true"], { on: true, at: "latched" }],
  ["Blackout: a hold is dark while held", "top.blackout", "alone", [{ is: "down" }, { is: "up", long: true }], ["Blackout on: true", "Blackout on: false"], { on: false, at: "off" }],
  ["Blackout: it is dark on the way down", "top.blackout", "alone", [{ is: "down" }], ["Blackout on: true"], { on: true, at: "flash" }],
  ["Blackout: a short press on a latched one brings the lights back", "top.blackout", "state:latched", [{ is: "down" }, { is: "up", long: false }], ["Blackout on: false"], { on: false, at: "off" }],
  ["Blackout: a long hold on a latched one changes nothing", "top.blackout", "state:latched", [{ is: "down" }, { is: "up", long: true }], [], { on: true, at: "latched" }],
  ["Blackout holds from Space", "top.blackout", "alone", [{ is: "key", down: true }, { is: "key", down: false }], ["Blackout on: true", "Blackout on: false"], { on: false, at: "off" }],
  ["LIVE turns on", "top.live", "alone", [{ is: "press" }], ["SetLive on: true"], { on: true }],
  ["LIVE turns off", "top.live", "live-on", [{ is: "press" }], ["SetLive on: false"], { on: false }],
  ["an indicator that is ready is turned off", "top.indicator.recording", "alone", [{ is: "press" }], ["SetRecording on: false"], { variant: "off", at: "off" }],
  ["an indicator in effect is turned off", "top.indicator.lock", "lock", [{ is: "press" }], ["SetLock on: false"], { variant: "off", at: "offLive" }],
  ["Learn waits for a pick, then is pressed again", "top.learn", "state:wait", [{ is: "down" }, { is: "up", long: false }], ["CancelLearn"], { on: false, at: "off" }],
  ["Learn pressed again in Learn mode ends Learn", "top.learn", "learn", [{ is: "down" }, { is: "up", long: false }], [], { on: false, at: "off" }],
  ["Learn held picks itself", "top.learn", "alone", [{ is: "down" }, { is: "up", long: true }], ["StartLearn"], { on: true, at: "wait" }],
  ["in Learn mode a press picks the control and does not fire it", "top.tap", "learn", [{ is: "press" }], ["StartLearn top.tap"], { learn: true }],
  ["two taps: the beat follows the taps", "top.tap", "alone", [{ is: "press" }, { is: "press" }], ["TapTempo", "TapTempo"], { at: "tap" }],
  ["the words 'from Tap' give the beat back to the feed", "top.tempo.source", "alone", [{ is: "press" }], ["ReleaseTempo"], { gone: "The feed drives the beat", at: "feed" }],
  ["Release all is off while nothing is held", "console.running.release-all", "alone", [{ is: "press" }], [], { off: "Nothing held" }],
  ["Release all gives back what is held", "console.running.release-all", "holding", [{ is: "press" }], ["ReleaseAll"], { off: "" }],
  ["a fader sends its level", "console.fader", "alone", [{ is: "set", value: 80, value2: 0 }], ["SetMaster 80"], { value: 80 }],
  ["the XY pad sends pan and tilt", "console.xy", "alone", [{ is: "set", value: 30, value2: 70 }], ["SetPosition pan 30, tilt 70"], { value: 30, value2: 70 }],
  ["a segment is picked", "console.speed", "alone", [{ is: "pick", option: "double" }], ["SetSpeed double"], { picked: "double" }],
  ["a tab changes only the screen", "top.workspaces", "alone", [{ is: "pick", option: "Songs" }], [], { picked: "Songs" }],
  ["a toggle with two commands: on, then off", "songs.play-from", "alone", [{ is: "press" }, { is: "press" }], ["PlayFrom", "StopPlayFrom"], { on: false }],
  ["a feed with no signal cannot be picked", "inout.tempo.feed", "state:off: no signal", [{ is: "pick", option: "Timecode" }], [], { off: "No signal", offOption: "Timecode" }],
  ["a feed with a signal can", "inout.tempo.feed", "state:off: no signal", [{ is: "pick", option: "Audio" }], ["SetTempoFeed Audio"], { picked: "Audio" }],
  ["a stepper at its end goes only the other way", "looks.seq.width", "state:narrower off at a quarter beat", [{ is: "step", by: -1 }, { is: "step", by: 1 }], ["SetSequenceEntry Wider"], { offOption: "Narrower" }],
  ["a drop sends once, with where it landed", "patch.grid.light", "alone", [{ is: "lift" }, { is: "drop" }], ["SetAddress where it landed"], { variant: "", at: "idle" }],
  ["a drag put back sends nothing", "patch.grid.light", "alone", [{ is: "lift" }, { is: "cancel" }], [], { variant: "", at: "idle" }],
  ["a lifted thing looks lifted", "looks.seq.bar", "alone", [{ is: "lift" }], [], { variant: "lifted", at: "dragging" }],
  ["the cross takes a look out of a section", "songs.section.remove-look", "alone", [{ is: "remove" }], ["RemoveSectionLook"], {}],
  ["Delete on a widget frame removes it", "console.widget.frame", "edit-surface", [{ is: "remove" }], ["RemoveWidget"], {}],
  ["Edit surface is gone while editing", "console.edit-surface", "alone", [{ is: "press" }], [], { gone: "'Done editing' is in its place", at: "editing" }],
  ["the add flow: yes adds the light and stops the flashing", "patch.add.confirm", "alone", [{ is: "press" }], ["AddFixture", "Identify"], { at: "find" }],
  ["the add flow: Next says where it leads", "patch.add.next", "alone", [{ is: "press" }], [], { label: "Next: flash it", at: "address" }],
  ["Stop engine is off while LIVE is on", "menubar.stop-engine", "live-on", [{ is: "press" }], [], { off: "LIVE is on" }],
  ["Stop engine works with the window closed", "menubar.stop-engine", "window-closed", [{ is: "press" }], ["StopEngine"], { gone: "" }],
  ["the menu bar item opens its menu and sends nothing", "menubar.item", "window-closed", [{ is: "press" }], [], { on: true }],
  ["a control of a screen is not there with the window closed", "top.blackout", "window-closed", [{ is: "press" }], [], { gone: "The window is closed" }],
];
for (const [what, id, key, gestures, sent, after] of plays) {
  test(`play: ${what}`, () => {
    const result = play(byId(id), frame(id, key).look, gestures);
    assert.deepEqual(result.sent.map((s) => `${s.name} ${s.value}`.trim()), sent);
    for (const [field, value] of Object.entries(after)) assert.deepEqual(result.look[field], value, field);
  });
}

test("the usual look of a control with a machine starts where the control lives in it", () => {
  assert.equal(usual(byId("console.pad")).at, "off");
  assert.equal(usual(byId("patch.add.confirm")).at, "flash");
  assert.equal(usual(byId("top.tempo.source")).at, "tap");
  assert.equal(usual(byId("top.undo")).at, "");
});

// The tables of the catalog point at rows, states and situations that exist.
test("samples, written states and uses name rows that exist", () => {
  const ids = new Set(CONTROLS.map((c) => c.id));
  const unknown = [...Object.keys(SAMPLES), ...Object.keys(STATE_LOOKS), ...Object.keys(SITUATION_STATE), ...Object.keys(USES)].filter((id) => !ids.has(id));
  assert.deepEqual(unknown, []);
  const states = Object.entries(STATE_LOOKS).flatMap(([id, looks]) => Object.keys(looks).filter((s) => !byId(id).states.includes(s)).map((s) => `${id}: ${s}`));
  assert.deepEqual(states, []);
  const places = Object.entries(SITUATION_STATE).flatMap(([id, by]) => Object.values(by).filter((s) => !byId(id).states.includes(s)).map((s) => `${id}: ${s}`));
  assert.deepEqual(places, []);
});

test("every control with a machine says how it uses it, and no other does", () => {
  const withMachine = CONTROLS.filter((c) => c.machine).map((c) => `${c.id}: ${c.machine}`);
  assert.deepEqual(Object.entries(USES).map(([id, use]) => `${id}: ${use.machine}`).sort(), withMachine.sort());
  const events = Object.entries(USES).flatMap(([id, use]) =>
    Object.values(use.on).flat().filter((event) => !(event in MACHINES[use.machine].events)).map((event) => `${id}: ${event}`),
  );
  assert.deepEqual(events, []);
});

// [query, ids found]
const searches = [
  ["", CONTROLS.length],
  ["blackout", ["top.blackout"]],
  ["BLACKOUT", ["top.blackout"]],
  ["menu bar", ["menubar.item", "menubar.open-window", "menubar.stop-engine"]],
  ["SetLive", ["top.live"]],
  ["top bar indicator lock", ["top.indicator.lock"]],
  ["no such thing", []],
];
for (const [query, expected] of searches) {
  test(`search: "${query}"`, () => {
    const found = search(CONTROLS, query).map((c) => c.id);
    if (typeof expected === "number") assert.equal(found.length, expected);
    else assert.deepEqual(found, expected);
  });
}

const KIT = Object.keys(PRIMITIVES);

test("the screens are tags, in the order of the table", () => {
  assert.equal(screensOf(CONTROLS)[0], "Top bar");
  assert.deepEqual([...screensOf(CONTROLS)].sort(), [...new Set(CONTROLS.map((c) => c.screen))].sort());
});

test("every control is drawn with a primitive of the kit and one of its kinds", () => {
  const known = new Set(KIT.flatMap(kitNames));
  assert.deepEqual(CONTROLS.filter((c) => !known.has(c.kit)).map((c) => `${c.id}: ${c.kit}`), []);
});

test("no control is a Status: a status is read, never pressed", () => {
  assert.deepEqual(CONTROLS.filter((c) => c.kit.startsWith("Status")).map((c) => c.id), []);
});

test("the index by function keeps every control, primitives in the order of the kit", () => {
  const groups = byFunction(CONTROLS, KIT);
  assert.deepEqual(groups.flatMap((g) => g.controls.map((c) => c.id)).sort(), CONTROLS.map((c) => c.id).sort());
  const order = groups.map((g) => KIT.indexOf(g.primitive));
  assert.deepEqual(order, [...order].sort((a, b) => a - b));
  assert.deepEqual(groups.slice(0, 3).map((g) => [g.primitive, g.kind]), [["Button", "action"], ["Button", "toggle"], ["Pad", ""]]);
});

// [control, primitive, kind, colour, fixed props, the primitives inside]
const recipes = [
  ["top.tap", "Button", "action", "plain", { fires: "down" }, []],
  ["top.blackout", "Button", "action", "danger", { fires: "down", held: "short press and hold", holds: "3 lines" }, []],
  ["top.learn", "Button", "toggle", "plain", { held: "short press and hold", holds: "3 lines" }, []],
  ["console.speed", "Picker", "segments", "plain", { fires: "down" }, []],
  ["songs.play-from", "Button", "toggle", "plain", { fires: "down", holds: "3 lines" }, []],
  ["top.live", "Button", "toggle", "plain", { mark: "knob", size: "large", holds: "2 lines" }, []],
  ["top.indicator.dj", "Button", "toggle", "plain", { mark: "lamp", quiet: "yes", lamp: "ok" }, []],
  ["top.indicator.autopilot", "Button", "toggle", "autopilot", { mark: "lamp" }, []],
  ["top.indicator.recording", "Button", "toggle", "danger", { mark: "lamp", quiet: "yes" }, []],
  ["console.pad", "Pad", "", "plain", {}, ["Status.flag", "Status.pill"]],
  ["console.autopilot.style", "Picker", "list", "plain", {}, []],
  ["looks.pattern.measure", "Field", "steps", "plain", {}, ["Button.action", "Status.value", "Button.action"]],
  ["songs.section.remove-look", "Item", "chip", "plain", {}, ["Button.action"]],
  ["top.now-strip", "Item", "strip", "plain", {}, ["Status.flag", "Status.pill"]],
  ["menubar.item", "Item", "entry", "plain", {}, ["Status.lamp"]],
  ["console.widget.resize", "Handle", "corner", "plain", {}, []],
];
for (const [id, primitive, kind, colour, props, inside] of recipes) {
  test(`recipe: ${id} is a ${primitive} ${kind}`, () => assert.deepEqual(recipe(byId(id)), { primitive, kind, colour, props, inside }));
}

// [control, the lines it holds the width for]
const lines = [
  ["top.blackout", ["Blackout", "Blackout on", "While held"]],
  ["top.live", ["LIVE off", "LIVE on"]],
  ["top.tap", []],
];
for (const [id, expected] of lines) test(`a control that changes its words keeps its width: ${id}`, () => assert.deepEqual(linesOf(byId(id)), expected));

// [what, steps, reviews after]
const A = "top.tap";
const reviews = [
  ["a rating", (r) => rate(r, A, 4), { [A]: { rating: 4, comment: "" } }],
  ["the same rating again takes it back", (r) => rate(rate(r, A, 4), A, 4), {}],
  ["another rating replaces it", (r) => rate(rate(r, A, 4), A, 2), { [A]: { rating: 2, comment: "" } }],
  ["a comment with no rating", (r) => comment(r, A, "too small"), { [A]: { rating: 0, comment: "too small" } }],
  ["a comment keeps the rating", (r) => comment(rate(r, A, 5), A, "good"), { [A]: { rating: 5, comment: "good" } }],
  ["an emptied comment with no rating is gone", (r) => comment(comment(r, A, "x"), A, " "), {}],
];
for (const [what, steps, expected] of reviews) test(`review: ${what}`, () => assert.deepEqual(steps({}), expected));

test("review: a control with no review reads as not rated", () => assert.deepEqual(reviewOf({}, A), { rating: 0, comment: "" }));

// [what, stored text, reviews read]
const stored = [
  ["nothing stored", null, {}],
  ["broken text", "{nope", {}],
  ["not an object", "7", {}],
  ["a review", '{"top.tap":{"rating":3,"comment":"ok"}}', { "top.tap": { rating: 3, comment: "ok" } }],
  ["a rating out of range is left out", '{"a":{"rating":9,"comment":""},"b":{"rating":1,"comment":""}}', { b: { rating: 1, comment: "" } }],
  ["a comment that is not text is left out", '{"a":{"rating":2,"comment":4},"b":null}', {}],
];
for (const [what, text, expected] of stored) test(`stored reviews: ${what}`, () => assert.deepEqual(parse(text), expected));

// [what, the file of the repository, this browser, both together]
const R = (rating, comment = "") => ({ rating, comment });
const meetings = [
  ["an empty browser takes the file", { a: R(3) }, {}, { a: R(3) }],
  ["a new file takes the browser", {}, { a: R(3) }, { a: R(3) }],
  ["both keep what only one has", { a: R(3) }, { b: R(5, "good") }, { a: R(3), b: R(5, "good") }],
  ["reviewed in both: the browser's stays", { a: R(3, "old") }, { a: R(4, "new") }, { a: R(4, "new") }],
];
for (const [what, file, browser, expected] of meetings) {
  test(`reviews meet: ${what}`, () => {
    assert.deepEqual(merge(file, browser), expected);
    assert.equal(same(merge(file, browser), file), JSON.stringify(expected) === JSON.stringify(file));
  });
}

// [what, the body put to the dev server, the text of the file]
const puts = [
  ["a map, names in order", '{"b":{"rating":1,"comment":""},"a":{"rating":2,"comment":"x"}}', '{\n  "a": {\n    "rating": 2,\n    "comment": "x"\n  },\n  "b": {\n    "rating": 1,\n    "comment": ""\n  }\n}\n'],
  ["an empty map", "{}", "{}\n"],
  ["broken text", "{nope", undefined],
  ["a list", "[]", undefined],
  ["not an object", "7", undefined],
];
for (const [what, body, expected] of puts) test(`the reviews file: ${what}`, () => assert.equal(fileText(body), expected));

test("the export is a table: primitives first, then controls in the order of the behaviour table", () => {
  const text = asMarkdown({ "top.tap": { rating: 4, comment: "fine | really\nyes" }, "top.live": { rating: 0, comment: "wider" }, [kitKey("Picker")]: { rating: 5, comment: "" } }, CONTROLS, KIT);
  assert.deepEqual(text.split("\n"), [
    `Catalog review: 2 of ${CONTROLS.length} controls, 1 of ${KIT.length} primitives`,
    "",
    "| Reviewed | Name | Where | Rating | Comment |",
    "|---|---|---|---|---|",
    "| Picker | `kit:Picker` | Kit | 5 of 5 |  |",
    "| LIVE switch | `top.live` | Top bar |  | wider |",
    "| Tap | `top.tap` | Top bar | 4 of 5 | fine \\| really yes |",
  ]);
});
