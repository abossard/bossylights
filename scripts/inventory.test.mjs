import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { areaOf, elementsOf, inventory } from "./inventory.mjs";
import { INVENTORY, inventoryText } from "./write-inventory.mjs";

const one = (source, spreading) => elementsOf("screens/x/index.tsx", source, spreading).map((e) => [e.tag, e.label, e.gestures.join("+"), e.sends.join(","), e.passes.join(","), e.ui.join(",")]);
const wrap = (jsx, before = "") => `${before}\nexport function Screen(props) { return ${jsx}; }`;

// [what, source, elements as [tag, label, gestures, sends, passes, ui]]
const cases = [
  ["a button with text", wrap(`<button onClick={() => store.tap()}>Tap</button>`), [["button", "Tap", "press", "tap", "", ""]]],
  ["a button is a control without a handler", wrap(`<button>Save</button>`), [["button", "Save", "press", "", "", ""]]],
  ["the accessible name wins over the text", wrap(`<button aria-label="Tap the tempo">Tap</button>`), [["button", "Tap the tempo", "press", "", "", ""]]],
  ["a computed name keeps its words", wrap("<button aria-label={`Remove ${w.title}`} />"), [["button", "Remove …", "press", "", "", ""]]],
  ["both words of a toggle", wrap(`<button>{on() ? "Muted" : "Mute"}</button>`), [["button", "Muted | Mute", "press", "", "", ""]]],
  ["text and a div are not controls", wrap(`<div class="a"><b>Hello</b></div>`), []],
  ["a div with a handler is", wrap(`<div onPointerDown={(e) => store.hold(1)} onPointerMove={move} />`), [["div", "", "drag+press and hold", "hold", "", ""]]],
  ["a role makes a control", wrap(`<span role="slider" tabIndex={0} aria-label="Edge" />`), [["span", "Edge", "", "", "", ""]]],
  ["an input says its type", wrap(`<input type="text" placeholder="Search" onInput={(e) => setQuery(e.currentTarget.value)} />`), [["input[text]", "Search", "type or pick", "", "", "setQuery"]]],
  ["a select", wrap(`<select aria-label="Style" onChange={(e) => store.setStyle(e.currentTarget.value)} />`), [["select", "Style", "type or pick", "setStyle", "", ""]]],
  ["a component with a handler", wrap(`<Fader label="Master" onChange={(v) => store.setMaster(v)} />`), [["Fader", "Master", "type or pick", "setMaster", "", ""]]],
  ["a component without one is not", wrap(`<Meter label="Level" value={1} />`), []],
  ["a callback of the component", wrap(`<button onClick={() => props.onOpen(1)}>Open</button>`), [["button", "Open", "press", "", "onOpen", ""]]],
  ["a handler by name is followed", wrap(`<button onClick={press}>Go</button>`, `const press = () => { store.start(1); setAt(2); };`), [["button", "Go", "press", "start", "", "setAt"]]],
  ["a made handler is followed", wrap(`<i onPointerDown={drag("move")} />`, `const drag = (how) => (e) => store.place(how);`), [["i", "", "press and hold", "place", "", ""]]],
  ["a method of a set is not a function of the file", wrap(`<button onClick={() => seen.add(1)}>Add</button>`, `const add = () => store.addThing();`), [["button", "Add", "press", "", "", ""]]],
  ["a key listener on the window", `export function installShortcuts() { window.addEventListener("keydown", (e) => store.undo()); }`, [["window", "keydown", "keyboard", "undo", "", ""]]],
  ["a component that hands its props on", wrap(`<Button disabled>Save</Button>`), [["Button", "Save", "", "", "", ""]], new Set(["Button"])],
  ["an svg has no words", wrap(`<button><svg><path d="M0 0" /></svg></button>`), [["button", "", "press", "", "", ""]]],
];
for (const [what, source, expected, spreading] of cases) {
  test(`inventory: ${what}`, () => assert.deepEqual(one(source, spreading), expected));
}

// [file, area]
for (const [file, area] of [["screens/console/widgets.tsx", "console"], ["shell/TopBar.tsx", "shell"], ["kit/Pad.tsx", "kit"], ["App.tsx", "app"]]) {
  test(`area of ${file}`, () => assert.equal(areaOf(file), area));
}

test("keys are stable names: area, component, what it says, numbered when two say the same", () => {
  const sources = {
    "screens/songs/index.tsx": wrap(`<><button>Mute</button><button>Mute</button><Region id="song"><button aria-label="Split the section now">x</button></Region></>`),
    "kit/Button.tsx": `export function Button(props) { return <button {...props} />; }`,
    "shell/TopBar.tsx": `function Status() { return <Button>Undo</Button>; }`,
  };
  assert.deepEqual(
    inventory(sources).map((e) => [e.key, e.region]),
    [
      ["kit/Button/button", ""],
      ["songs/Screen/mute", ""],
      ["songs/Screen/mute#2", ""],
      ["songs/Screen/split-the-section-now", "song"],
      ["shell/Status/undo", ""],
    ],
  );
});

test(`${INVENTORY} is what the mockups hold today (run: node scripts/write-inventory.mjs)`, () => {
  assert.equal(readFileSync(INVENTORY, "utf8"), inventoryText());
});

test("docs/design/controls.md is what the table says today (run: node scripts/write-controls-doc.mjs)", async () => {
  const { DOC, docText } = await import("./write-controls-doc.mjs");
  assert.equal(readFileSync(DOC, "utf8"), docText());
});

test("docs/design/kit.md is what the table and the recipes say today (run: node scripts/write-kit-doc.mjs)", async () => {
  const { DOC, docText } = await import("./write-kit-doc.mjs");
  assert.equal(readFileSync(DOC, "utf8"), docText());
});
