import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { act } from "./catalog/act.ts";
import { byFunction, linesOf, recipe } from "./catalog/compose.ts";
import { framesOf, screensOf, search } from "./catalog/frames.ts";
import { inSituation, inState, usual } from "./catalog/look.ts";
import { place } from "./catalog/place.ts";
import { arrived, asMarkdown, comment, kitKey, merge, parse, rate, reviewOf, same } from "./catalog/review.ts";
import { fileText, withReview } from "./catalog/reviews-file.mjs";
import { SAMPLES, SITUATION_STATE, STATE_LOOKS } from "./catalog/samples.ts";
import { SHAPES } from "./catalog/shapes.ts";
import { atLimit } from "./catalog/steps.ts";
import { USES } from "./catalog/uses.ts";
import { MACHINES } from "./controls/machines.ts";
import { SITUATIONS } from "./controls/situations.ts";
import { ADDRESSES, kitNames, PRIMITIVES, UNIVERSES, WIDTH_SIXTEENTHS } from "./controls/spec.ts";
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
  ["Release all is off while nothing is held", "console.running.release-all", "alone", [{ is: "press" }], [], { off: "Nothing held" }],
  ["Release all gives back what is held", "console.running.release-all", "holding", [{ is: "press" }], ["ReleaseAll"], { off: "" }],
  ["a fader sends its level", "console.fader", "alone", [{ is: "set", value: 80, value2: 0 }], ["SetMaster 80"], { value: 80 }],
  ["the XY pad sends pan and tilt", "console.xy", "alone", [{ is: "set", value: 30, value2: 70 }], ["SetPosition pan 30, tilt 70"], { value: 30, value2: 70 }],
  ["a segment is picked", "console.speed", "alone", [{ is: "pick", option: "double" }], ["SetSpeed double"], { picked: "double" }],
  ["a tab changes only the screen", "top.workspaces", "alone", [{ is: "pick", option: "Songs" }], [], { picked: "Songs" }],
  ["a toggle with two commands: on, then off", "songs.play-from", "alone", [{ is: "press" }, { is: "press" }], ["PlayFrom", "StopPlayFrom"], { on: false }],
  ["a feed with no signal cannot be picked", "inout.tempo.feed", "state:off: no signal", [{ is: "pick", option: "Timecode" }], [], { off: "No signal", offOption: "Timecode" }],
  ["a feed with a signal can", "inout.tempo.feed", "state:off: no signal", [{ is: "pick", option: "Audio" }], ["SetTempoFeed Audio"], { picked: "Audio" }],
  ["a stepper at its end goes only the other way", "looks.seq.width", "state:narrower off at a quarter beat", [{ is: "step", by: -1 }, { is: "step", by: 1 }], ["SetSequenceEntry ½ beat"], { offOption: "", sub: "½ beat" }],
  ["a drop sends once, with where it landed", "patch.grid.light", "alone", [{ is: "lift" }, { is: "drop", by: { across: 1, down: 0, wider: 0 } }], ["SetAddress 18 to 24"], { variant: "", at: "idle", sub: "18 to 24" }],
  ["a drop of a thing that is not placed says it landed", "console.widget.frame", "edit-surface", [{ is: "lift" }, { is: "drop" }], ["PlaceWidget where it landed"], { at: "idle" }],
  ["an arrow moves a placed bar and sends where it is", "looks.seq.bar", "alone", [{ is: "nudge", by: { across: 1, down: 0, wider: 0 } }], ["SetSequenceEntry At beat 1¼, 1 beat"], { sub: "At beat 1¼, 1 beat" }],
  ["an edge dropped where it started sends nothing", "songs.section.edge", "alone", [{ is: "lift" }, { is: "drop", by: { across: 0, down: 0, wider: 0 } }], [], { at: "idle" }],
  ["an edge dropped elsewhere says it landed", "songs.section.edge", "alone", [{ is: "lift" }, { is: "drop", by: { across: 2, down: 0, wider: 0 } }], ["MoveSectionEdge where it landed"], { at: "idle" }],
  ["a drop where it started sends nothing", "looks.seq.bar", "alone", [{ is: "lift" }, { is: "drop", by: { across: -4, down: 0, wider: 0 } }], [], { variant: "", at: "idle", sub: "At beat 1, 1 beat" }],
  ["an arrow that cannot move it sends nothing", "looks.seq.bar", "alone", [{ is: "nudge", by: { across: -1, down: 0, wider: 0 } }], [], { sub: "At beat 1, 1 beat" }],
  ["a test of an output does nothing while it runs", "inout.out.test", "alone", [{ is: "press" }, { is: "press" }], ["TestOutput on: true"], { label: "Testing" }],
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
  // #189
  ["UX-03 a press opens the look and marks it", "looks.library.look", "alone", [{ is: "press" }], [], { on: true, refused: "" }],
  ["UX-03 a press without the engine still opens the look", "looks.library.look", "no-engine", [{ is: "press" }], [], { on: true }],
  ["UX-03 a look dropped on a group joins it", "looks.library.look", "alone", [{ is: "lift" }, { is: "drop", by: { across: 0, down: 2, wider: 0 } }], ["AddToGroup where it landed"], { refused: "" }],
  ["UX-03 without the engine the drop is refused", "looks.library.look", "no-engine", [{ is: "lift" }, { is: "drop", by: { across: 0, down: 2, wider: 0 } }], [], { refused: "Engine not connected", variant: "", at: "idle" }],
  ["UX-03 the refusal goes with the next gesture", "looks.library.look", "no-engine", [{ is: "lift" }, { is: "drop", by: { across: 0, down: 2, wider: 0 } }, { is: "press" }], [], { refused: "", on: true }],
  ["UX-06 Escape ends Learn while it waits for a pick", "top.learn", "state:pick", [{ is: "cancel" }], [], { on: false, at: "off" }],
  ["UX-06 Escape ends Learn while it waits for the hardware", "top.learn", "state:wait", [{ is: "cancel" }], ["CancelLearn"], { on: false, at: "off" }],
  ["UX-06 Escape ends Learn in a control that waits to be picked", "top.tap", "learn", [{ is: "cancel" }], [], { learn: false }],
  ["UX-19 Wider adds a quarter beat and sends the width", "looks.seq.width", "alone", [{ is: "step", by: 1 }], ["SetSequenceEntry 1¼ beats"], { sub: "1¼ beats", off: "" }],
  ["UX-19 Narrower down to a quarter beat, then it is off", "looks.seq.width", "alone", [{ is: "step", by: -1 }, { is: "step", by: -1 }, { is: "step", by: -1 }, { is: "step", by: -1 }], ["SetSequenceEntry ¾ beat", "SetSequenceEntry ½ beat", "SetSequenceEntry ¼ beat"], { sub: "¼ beat", off: "A quarter beat is the least", offOption: "Narrower" }],
  ["UX-19 ×2 doubles the measure", "looks.pattern.measure", "alone", [{ is: "step", by: 1 }], ["SetPattern 8 beats"], { sub: "8 beats" }],
  ["UX-19 ×2 stops at 32 beats", "looks.pattern.measure", "alone", [{ is: "step", by: 1 }, { is: "step", by: 1 }, { is: "step", by: 1 }, { is: "step", by: 1 }], ["SetPattern 8 beats", "SetPattern 16 beats", "SetPattern 32 beats"], { sub: "32 beats", off: "32 beats is the most", offOption: "×2" }],
  ["UX-19 ÷2 at 1 beat is off", "looks.pattern.measure", "state:÷2 off at 1", [{ is: "step", by: -1 }], [], { sub: "1 beat" }],
  ["UX-19 Higher moves the light one address up", "patch.details.address", "alone", [{ is: "step", by: 1 }], ["SetAddress 18"], { sub: "18" }],
  ["UX-19 the proposed address changes and sends nothing", "patch.add.address", "alone", [{ is: "step", by: -1 }], [], { sub: "32" }],
  ["UX-25 the autopilot turned on says no more that it waits", "console.autopilot.on", "live-on", [], [], { sub: "" }],
  ["UX-26 Use this style goes once it is used", "looks.autopilot.use-style", "alone", [{ is: "press" }, { is: "press" }], ["SetAutopilotStyle"], { gone: "While in use" }],
  ["UX-26 the beat back from Tap names the feed", "top.tempo.source", "alone", [{ is: "press" }], ["ReleaseTempo"], { label: "from VirtualDJ", gone: "", at: "feed", words: true, off: "" }],
  ["UX-26 with the feed the words are not pressed", "top.tempo.source", "state:from the feed", [{ is: "press" }], [], { label: "from VirtualDJ", words: true }],
  ["UX-33 the explainer pad stops on a second short press", "how.pad", "alone", [{ is: "down" }, { is: "up", long: false }, { is: "down" }, { is: "up", long: false }], [], { variant: "idle", on: false }],
  ["UX-33 the explainer pad flashes its own model and sends nothing", "how.pad", "alone", [{ is: "down" }, { is: "up", long: true }], [], { variant: "idle", on: false }],
  ["UX-33 the walk steps on and starts again", "how.walk.step", "alone", [{ is: "press" }], [], { label: "Next", sub: "Step 1 of 4: Press" }],
  ["UX-33 the walk ends with Again", "how.walk.step", "alone", [{ is: "press" }, { is: "press" }, { is: "press" }, { is: "press" }], [], { label: "Again", sub: "Step 4 of 4: Lights" }],
  ["UX-33 Again starts the walk over at Step 1", "how.walk.step", "alone", [{ is: "press" }, { is: "press" }, { is: "press" }, { is: "press" }, { is: "press" }], [], { label: "Next", sub: "Step 1 of 4: Press" }],
  ["UX-33 the trace shows the steps it played", "modules.trace", "alone", [{ is: "press" }], [], { sub: "Played: Controller, Playback, Mixer, Output" }],
];
for (const [what, id, key, gestures, sent, after] of plays) {
  test(`play: ${what}`, () => {
    const result = play(byId(id), frame(id, key).look, gestures);
    assert.deepEqual(result.sent.map((s) => `${s.name} ${s.value}`.trim()), sent);
    for (const [field, value] of Object.entries(after)) assert.deepEqual(result.look[field], value, field);
  });
}

// #189: [what, id, frame, field, expected]
const facts = [
  ["UX-10 the shapes of a pattern", "looks.pattern.shape", "alone", "options", ["Plasma", "Stripes", "Wave", "Sparkle"]],
  ["UX-10 a pattern runs on a group or a matrix", "looks.pattern.runs-on", "alone", "options", ["PARs", "Movers", "Panel"]],
  ["UX-25 the autopilot waits for LIVE only while ready", "console.autopilot.on", "state:on", "sub", ""],
  ["UX-25 off says nothing about LIVE", "console.autopilot.on", "state:off", "sub", ""],
  ["UX-25 ready waits for LIVE", "console.autopilot.on", "alone", "sub", "Waits for LIVE"],
  ["UX-27 every colour order", "patch.panel.order", "alone", "options", ["RGB", "RBG", "GRB", "GBR", "BRG", "BGR"]],
  ["UX-27 the three walks", "how.walk", "alone", "options", ["Press a pad", "Record and replay", "The autopilot yields"]],
  ["UX-28 the tab of a lost output is marked", "dmx.universe", "output-lost", "variant", "marked"],
  ["UX-29 the universe search says what it searches", "patch.universes.search", "alone", "label", "Search universes"],
  ["UX-29 the drawer adds a pad", "console.drawer.add", "alone", "label", "Pads"],
  ["UX-19 Narrower at its end shows a quarter beat", "looks.seq.width", "state:narrower off at a quarter beat", "sub", "¼ beat"],
  ["UX-19 Narrower at its end says why", "looks.seq.width", "state:narrower off at a quarter beat", "off", "A quarter beat is the least"],
  ["UX-19 ×2 at its end shows 32 beats", "looks.pattern.measure", "state:×2 off at 32", "sub", "32 beats"],
];
for (const [what, id, key, field, expected] of facts) test(what, () => assert.deepEqual(frame(id, key).look[field], expected));

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
  ["menu bar", ["top.show", "menubar.item", "menubar.open-window", "menubar.stop-engine"]],
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
  ["top.indicator.recording", "Button", "toggle", "danger", { mark: "lamp", quiet: "yes", holds: "2 lines" }, []],
  ["console.pad", "Pad", "", "plain", {}, ["Status.flag", "Status.pill"]],
  ["console.xy", "XY", "position", "plain", {}, []],
  ["looks.step.colour.wheel", "XY", "colour", "plain", {}, []],
  ["looks.step.gobo", "Picker", "pictures", "plain", {}, []],
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

// [control, readout, across, down, wider, readout after]
const places = [
  ["patch.grid.light", "17 to 23", 1, 0, 0, "18 to 24"],
  ["patch.grid.light", "17 to 23", 0, 1, 0, "49 to 55"],
  ["patch.grid.light", "1 to 7", -1, 0, 0, "1 to 7"],
  ["patch.grid.light", "500 to 506", 0, 1, 0, "506 to 512"],
  ["patch.grid.light", "17 to 23", 0, 0, 1, "17 to 23"],
  ["looks.seq.bar", "At beat 1, 1 beat", 1, 0, 0, "At beat 1¼, 1 beat"],
  ["looks.seq.bar", "At beat 1, 1 beat", 4, 0, 0, "At beat 2, 1 beat"],
  ["looks.seq.bar", "At beat 1, 1 beat", -1, 0, 0, "At beat 1, 1 beat"],
  ["looks.seq.bar", "At beat 2½, ¼ beat", 0, 0, -1, "At beat 2½, ¼ beat"],
  ["looks.seq.bar", "At beat 1, 1 beat", 0, 0, 2, "At beat 1, 1½ beats"],
  ["looks.seq.bar", "At beat 1, 1 beat", 0, 3, 0, "At beat 1, 1 beat"],
  ["looks.seq.bar", "1 beat", 1, 0, 0, undefined],
  ["console.widget.frame", "Main", 1, 0, 0, undefined],
];
for (const [id, readout, across, down, wider, after] of places) {
  test(`place: ${id} at ${readout} by ${across}, ${down}, ${wider}`, () => assert.equal(place(id, readout, { across, down, wider }), after));
}

// [control, text, the reason it is refused: undefined when it fits]
const shapes = [
  ["inout.osc.port", "9000", undefined],
  ["inout.osc.port", "65536", "A port is a number from 1 to 65535"],
  ["inout.osc.port", "0", "A port is a number from 1 to 65535"],
  ["inout.osc.port", "Banana", "A port is a number from 1 to 65535"],
  ["inout.out.universes", "1", undefined],
  ["inout.out.universes", "2 to 9", undefined],
  ["inout.out.universes", "9 to 2", "One universe from 1 to 64, as 1, or a range, as 2 to 9"],
  ["inout.out.universes", "0", "One universe from 1 to 64, as 1, or a range, as 2 to 9"],
  ["inout.mapping.message", "Note 36", undefined],
  ["inout.mapping.message", "CC 127", undefined],
  ["inout.mapping.message", "CC 128", "A note, as Note 36, or a controller, as CC 48"],
  ["inout.mapping.message", "note 36", "A note, as Note 36, or a controller, as CC 48"],
  ["patch.panel.start", "2", undefined],
  ["patch.panel.start", "0", "A universe is a number from 1 to 64"],
  ["patch.panel.start", "64", undefined],
  ["patch.panel.start", "65", "A universe is a number from 1 to 64"],
  ["inout.out.universes", "64", undefined],
  ["inout.out.universes", "2 to 64", undefined],
  ["inout.out.universes", "65", "One universe from 1 to 64, as 1, or a range, as 2 to 9"],
  ["inout.out.universes", "2 to 65", "One universe from 1 to 64, as 1, or a range, as 2 to 9"],
  ["inout.out.universes", "1 to 32768", "One universe from 1 to 64, as 1, or a range, as 2 to 9"],
  ["inout.out.universes", "99999999", "One universe from 1 to 64, as 1, or a range, as 2 to 9"],
  ["patch.panel.size", "16 x 16", undefined],
  ["patch.panel.size", "10880 x 1", undefined],
  ["patch.panel.size", "1 x 10880", undefined],
  ["patch.panel.size", "105 x 104", "Columns x rows, as 16 x 16, at most 10880 pixels"],
  ["patch.panel.size", "1024 x 1024", "Columns x rows, as 16 x 16, at most 10880 pixels"],
  ["patch.panel.size", "Banana", "Columns x rows, as 16 x 16, at most 10880 pixels"],
  ["patch.panel.size", "0 x 16", "Columns x rows, as 16 x 16, at most 10880 pixels"],
  ["patch.panel.size", "-3", "Columns x rows, as 16 x 16, at most 10880 pixels"],
];
for (const [id, text, reason] of shapes) test(`shape: ${id} takes "${text}"`, () => assert.equal(SHAPES[id]?.(text), reason));

test("the limits of the fields are the engine's: crates/engine-core/src/channel.rs", () => {
  const rust = readFileSync(new URL("../../crates/engine-core/src/channel.rs", import.meta.url), "utf8");
  const constant = (name) => Number(new RegExp(`pub const ${name}: \\w+ = (\\d+);`).exec(rust)?.[1]);
  assert.deepEqual([UNIVERSES, ADDRESSES], [constant("MAX_UNIVERSES"), constant("ADDRESSES")]);
});

test("an item's width is the engine's: u16 sixteenths of a beat in crates/engine-core/src/api.rs", () => {
  const rust = readFileSync(new URL("../../crates/engine-core/src/api.rs", import.meta.url), "utf8");
  assert.match(rust, /Width is counted in sixteenths of a beat/);
  assert.match(rust, /pub struct SequenceItem \{[^}]*pub width: u16,/);
  assert.equal(WIDTH_SIXTEENTHS, 2 ** 16 - 1);
});

// The steppers at their upper limit: the last address the 7 channels of the sample light fit at, the widest item. [id, buttons, the look there]
const highest = [
  ["patch.details.address", ["Lower", "Higher"], { sub: String(ADDRESSES - 7 + 1), off: "Last address that fits", offOption: "Higher" }],
  ["patch.add.address", ["Lower", "Higher"], { sub: String(ADDRESSES - 7 + 1), off: "Last address that fits", offOption: "Higher" }],
  ["looks.seq.width", ["Narrower", "Wider"], { sub: "4095¾ beats", off: "4095¾ beats is the most", offOption: "Wider" }],
];
for (const [id, buttons, expected] of highest) test(`UX-19 ${id} at its upper limit`, () => assert.deepEqual(atLimit(id, 1, buttons), expected));

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

// UX-12: another tab changed the store from one text to the next. Only what it changed comes in. [what, this tab, store before, store after, this tab after]
const arrivals = [
  ["a review only in the file stays", { a: R(4, "from the file") }, {}, { b: R(2) }, { a: R(4, "from the file"), b: R(2) }],
  ["a changed review comes in", { a: R(4) }, { a: R(4) }, { a: R(5, "better") }, { a: R(5, "better") }],
  ["a review taken out goes", { a: R(4), b: R(2) }, { a: R(4), b: R(2) }, { a: R(4) }, { a: R(4) }],
  ["what the other tab left alone keeps this tab's", { a: R(3, "newer here") }, { a: R(1) }, { a: R(1), b: R(2) }, { a: R(3, "newer here"), b: R(2) }],
];
for (const [what, now, before, after, expected] of arrivals) test(`UX-12 another tab: ${what}`, () => assert.deepEqual(arrived(now, before, after), expected));

// [what, the body put to the dev server, the text of the file]
const puts = [
  ["a map, names in order", '{"b":{"rating":1,"comment":""},"a":{"rating":2,"comment":"x"}}', '{\n  "a": {\n    "rating": 2,\n    "comment": "x"\n  },\n  "b": {\n    "rating": 1,\n    "comment": ""\n  }\n}\n'],
  ["an empty map", "{}", "{}\n"],
  ["broken text", "{nope", undefined],
  ["a list", "[]", undefined],
  ["not an object", "7", undefined],
];
for (const [what, body, expected] of puts) test(`the reviews file: ${what}`, () => assert.equal(fileText(body), expected));

// UX-12: one review at a time, put into the file as it is now. [what, file, body, file after]
const ones = [
  ["a review joins the others", { a: R(2) }, { id: "b", review: R(3, "ok") }, { a: R(2), b: R(3, "ok") }],
  ["a review replaces its old one only", { a: R(2), b: R(1) }, { id: "b", review: R(5) }, { a: R(2), b: R(5) }],
  ["a review taken back leaves the file", { a: R(2), b: R(1) }, { id: "b", review: null }, { a: R(2) }],
  ["a broken file is an empty one", "{nope", { id: "b", review: R(4) }, { b: R(4) }],
  ["a body with no name is refused", { a: R(2) }, { review: R(4) }, undefined],
  ["a body with a broken review is refused", { a: R(2) }, { id: "b", review: { rating: 9, comment: "" } }, undefined],
];
for (const [what, file, body, expected] of ones) {
  test(`the reviews file, one review: ${what}`, () => {
    const after = withReview(typeof file === "string" ? file : JSON.stringify(file), JSON.stringify(body));
    assert.deepEqual(after === undefined ? undefined : JSON.parse(after), expected);
  });
}

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
