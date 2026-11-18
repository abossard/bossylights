// The behaviour table as a document: docs/design/controls.md, written from ui/src/controls (issue #102).
// A calculation: the data in, Markdown out. `write-controls-doc.mjs` writes the file.
import { COMMANDS } from "../ui/src/controls/commands.ts";
import { CONTRADICTIONS, MISSING } from "../ui/src/controls/findings.ts";
import { MACHINES } from "../ui/src/controls/machines.ts";
import { GESTURES, SITUATIONS } from "../ui/src/controls/situations.ts";
import { USUAL } from "../ui/src/controls/spec.ts";
import { CONTROLS, NOT_CONTROLS } from "../ui/src/controls/table.ts";

const cellText = (text) => String(text).replace(/\|/g, "\\|").replace(/\n/g, " ");
const row = (cells) => `| ${cells.map(cellText).join(" | ")} |`;
const table = (head, rows) => [row(head), row(head.map(() => "---")), ...rows.map(row)].join("\n");
const code = (text) => `\`${text}\``;

const MARK = { works: "●", off: "○", gone: "–" };
/** A cell as one mark: ● the usual of its kind, ◆ works but differs, ○ off, – not there. */
const mark = (control, situation) => {
  const cell = control.cells[situation.id];
  const usual = USUAL[control.kind][situation.id];
  if (cell.is === "works" && (cell.says !== usual.says || usual.is !== "works")) return "◆";
  return MARK[cell.is];
};
const differs = (control, situation) => {
  const cell = control.cells[situation.id];
  const usual = USUAL[control.kind][situation.id];
  return cell.is !== usual.is || cell.says !== usual.says;
};
const IS = { works: "works", off: "off", gone: "not there" };
const KINDS = { play: "plays the lights, now, by hand", edit: "changes the show file", setting: "changes how the engine behaves", view: "changes only what the screen shows" };
const VERDICT = { keep: "keep", change: "change", remove: "remove" };

function machineDiagram(machine) {
  const id = (state) => state.replace(/[^A-Za-z0-9]/g, "");
  const lines = ["```mermaid", "stateDiagram-v2", "  direction LR", `  [*] --> ${id(machine.start)}`];
  for (const [from, events] of Object.entries(machine.table)) {
    const byTarget = new Map();
    for (const [event, step] of Object.entries(events)) {
      if (step.to === from && !(step.sends ?? []).length) continue;
      const label = `${event}${(step.sends ?? []).length ? ` / ${step.sends.join(", ")}` : ""}`;
      byTarget.set(step.to, [...(byTarget.get(step.to) ?? []), label]);
    }
    for (const [to, labels] of byTarget) lines.push(`  ${id(from)} --> ${id(to)}: ${labels.join(", ")}`);
  }
  lines.push("```");
  return lines.join("\n");
}

function machineSection(name, machine, controls) {
  const users = controls.filter((c) => c.machine === name).map((c) => code(c.id));
  const notes = Object.entries(machine.table).flatMap(([from, events]) =>
    Object.entries(events).filter(([, step]) => step.note).map(([event, step]) => [from, event, step.to, step.note]),
  );
  return [
    `### ${machine.title}`,
    "",
    `Used by: ${users.join(", ")}`,
    "",
    machineDiagram(machine),
    "",
    table(["State", "On screen"], Object.entries(machine.states)),
    "",
    table(["Event", "Caused by"], Object.entries(machine.events)),
    ...(notes.length ? ["", table(["From", "Event", "To", "Note"], notes)] : []),
    "",
    "<details><summary>Every pair of state and event</summary>",
    "",
    table(
      ["State", ...Object.keys(machine.events)],
      Object.entries(machine.table).map(([from, events]) => [
        from,
        ...Object.values(events).map((step) => (step.to === from ? "stays" : `to ${step.to}`) + ((step.sends ?? []).length ? `, sends ${step.sends.join(", ")}` : "")),
      ]),
    ),
    "",
    "</details>",
  ].join("\n");
}

function screenSection(screen, controls) {
  const rows = controls.filter((c) => c.screen === screen);
  const different = rows.flatMap((c) => SITUATIONS.filter((s) => differs(c, s)).map((s) => [c.name, s.name, IS[c.cells[s.id].is], c.cells[s.id].says]));
  return [
    `## ${screen}`,
    "",
    `${rows.length} controls.`,
    "",
    table(
      ["Control", "Name", "Does", "Sends", "Kind"],
      rows.map((c) => [c.name, code(c.id), c.does, c.sends.length ? c.sends.map(code).join(" ") : `nothing: ${c.ui}`, c.kind]),
    ),
    "",
    `### ${screen}: in each situation`,
    "",
    table(["Control", ...SITUATIONS.map((s) => s.name)], rows.map((c) => [c.name, ...SITUATIONS.map((s) => mark(c, s))])),
    "",
    `### ${screen}: where a cell is not the usual of its kind`,
    "",
    table(["Control", "Situation", "", "What happens, or the reason shown"], different),
    "",
    `### ${screen}: gestures`,
    "",
    table(["Control", ...GESTURES.map((g) => g.charAt(0).toUpperCase() + g.slice(1))], rows.map((c) => [c.name, ...GESTURES.map((g) => c.gestures[g])])),
    "",
    `### ${screen}: feedback, undo, Learn, Function first`,
    "",
    table(
      ["Control", "On screen", "On a mapped controller", "Undo", "Learn", "Function first"],
      rows.map((c) => [
        c.name,
        c.feedback.screen,
        c.feedback.controller,
        `${c.undo.yes ? "yes" : "no"}. ${c.undo.why}`,
        `${c.learn.yes ? "yes" : "no"}. ${c.learn.why}`,
        `**${VERDICT[c.first.verdict]}**. ${c.first.why}`,
      ]),
    ),
  ].join("\n");
}

/** docs/design/controls.md as text. `inventory`: the elements of `inventory.json`. */
export function controlsDoc(inventory) {
  const screens = [...new Set(CONTROLS.map((c) => c.screen))];
  const verdicts = (v) => CONTROLS.filter((c) => c.first.verdict === v);
  const not = Object.entries(NOT_CONTROLS);
  const notBy = (is) => not.filter(([, n]) => n.is === is);
  const commands = Object.entries(COMMANDS);
  const senders = (name) => CONTROLS.filter((c) => c.sends.includes(name)).map((c) => code(c.id));
  const KIND_OF = { "two-one": "Two controls for one thing", "one-two": "One control or one word, two meanings", "no-reason": "Off with no reason shown", rule: "Against a rule of the product" };

  return [
    "# Every control, in every situation",
    "",
    "Written by `node scripts/write-controls-doc.mjs` from `ui/src/controls`. Change the data there, never this file.",
    "This is the paper form of the storyboard catalog of issue #102. The catalog shows the same rows as live frames: `npm run catalog`.",
    "",
    "```mermaid",
    "flowchart LR",
    `  M["8 mockups<br/>${inventory.length} elements in the code"] --> I["Inventory<br/>by script"]`,
    `  I --> R["${CONTROLS.length} controls<br/>one row each"]`,
    `  I --> N["${not.length} not controls<br/>each with a reason"]`,
    `  R --> C["${commands.length} commands"]`,
    `  R --> S["${Object.keys(MACHINES).length} state machines"]`,
    `  R --> F["Function first<br/>${verdicts("keep").length} keep, ${verdicts("change").length} change, ${verdicts("remove").length} remove"]`,
    "```",
    "",
    table(
      ["Where", "What"],
      [
        ["`ui/src/controls/inventory.json`", "every interactive element of the mockups, found by `scripts/inventory.mjs`"],
        ["`ui/src/controls/rows/`", "the rows, one file per screen"],
        ["`ui/src/controls/situations.ts`", "the situations, the columns"],
        ["`ui/src/controls/commands.ts`", "the commands"],
        ["`ui/src/controls/machines.ts`", "the state machines"],
        ["`ui/src/controls/findings.ts`", "contradictions and missing controls"],
        ["`ui/src/controls.test.mjs`", "no empty cell, every element has a row, every command is sent, every state is reached"],
        ["`ui/src/kit/`", "ten primitives the controls are drawn with, named by what they are for: props in, a gesture out, no engine. Which one draws which control: `docs/design/kit.md`"],
        ["`ui/src/catalog/`", "the catalog: the primitives first, then one page per control with a live frame per situation and state, the command as it fires, rating and comment"],
      ],
    ),
    "",
    "## How to read a row",
    "",
    table(
      ["Rule", ""],
      [
        ["A column is one fact", "Each situation is the usual state plus one fact. The usual state: LIVE off, engine connected, lights patched, nothing held, no song, autopilot off, every output running"],
        ["A control is checked where it exists", "'Release' on one look is checked with a look held. A control is 'not there' only when the fact of the column removes it"],
        ["Four kinds", Object.entries(KINDS).map(([k, v]) => `**${k}** ${v}`).join(". ")],
        ["A row says only where it differs", "Each kind has a usual answer per situation, in the next table. A row states the cells that differ"],
        ["Off means: with its reason", "A control that is off shows the reason under its label. The reason is the text of the cell, 40 letters at most"],
        ["Marks", "● the usual answer of its kind. ◆ works, with its own answer. ○ off, with the reason. – not there"],
      ],
    ),
    "",
    "## The situations",
    "",
    table(["Situation", "The one fact"], SITUATIONS.map((s) => [s.name, s.fact])),
    "",
    "## The usual answer of each kind",
    "",
    table(
      ["Situation", ...Object.keys(KINDS)],
      SITUATIONS.map((s) => [s.name, ...Object.keys(KINDS).map((k) => (USUAL[k][s.id].is === "works" ? "" : `**${IS[USUAL[k][s.id].is]}**: `) + USUAL[k][s.id].says)]),
    ),
    "",
    "## Function first",
    "",
    "The test of `DESIGN.md`, run on every control. Proposals until the owner rates them in the catalog.",
    "",
    `### Removed: ${verdicts("remove").length}`,
    "",
    table(["Control", "Screen", "Why it goes"], verdicts("remove").map((c) => [`${c.name} ${code(c.id)}`, c.screen, c.first.why])),
    "",
    `### Changed: ${verdicts("change").length}`,
    "",
    table(["Control", "Screen", "What changes"], verdicts("change").map((c) => [`${c.name} ${code(c.id)}`, c.screen, c.first.why])),
    "",
    `## Contradictions: ${CONTRADICTIONS.length}, each resolved`,
    "",
    "A number in the last column is a question on the owner's decisions page.",
    "",
    ...Object.entries(KIND_OF).flatMap(([kind, title]) => [
      `### ${title}`,
      "",
      table(
        ["What", "Rows", "Resolved", "Asked"],
        CONTRADICTIONS.filter((c) => c.kind === kind).map((c) => [c.what, c.where.map(code).join(" "), c.resolved, c.asks ?? ""]),
      ),
      "",
    ]),
    `## Missing: ${MISSING.length} things with no control in any mockup`,
    "",
    "Found while writing the rows. Each needs a control before the screen it belongs to can be built, or a 'not in version 1'.",
    "",
    table(["Screen", "What cannot be done", "Engine command that waits"], MISSING.map((m) => [m.screen, m.what, m.command ?? ""])),
    "",
    `## Commands: ${commands.length}`,
    "",
    "The union of what the controls send: the first list of engine commands. A proposal for the engine, not a design of it.",
    "",
    table(
      ["Command", "Does", "Fields", "In the engine today", "Sent by"],
      commands.map(([name, spec]) => [code(name), spec.does, spec.fields, "engine" in spec ? "yes" : "", senders(name).join(" ") || (Object.values(MACHINES).some((m) => JSON.stringify(m.table).includes(`"${name}"`)) ? "a state machine" : "no control: see Missing")]),
    ),
    "",
    `## State machines: ${Object.keys(MACHINES).length}`,
    "",
    "A machine is drawn where what a press does depends on what happened before. A choice among a few values, and a row that shows what kind of thing it lists, are not machines.",
    "Arrows are the pairs that change the state or send a command. The table under each diagram holds every pair.",
    "",
    ...Object.entries(MACHINES).flatMap(([name, machine]) => [machineSection(name, machine, CONTROLS), ""]),
    ...screens.flatMap((screen) => [screenSection(screen, CONTROLS), ""]),
    `## Not controls: ${not.length} elements of the mockups`,
    "",
    table(
      ["Why", "How many", "Elements"],
      [
        ["The inside of a component, counted where the component is used", notBy("kit").length, notBy("kit").map(([k]) => code(k)).join(" ")],
        ["A component that only hands callbacks on", notBy("wiring").length, notBy("wiring").map(([k]) => code(k)).join(" ")],
        ["Tooling of the mockups, not of the product", notBy("tooling").length, notBy("tooling").map(([k]) => code(k)).join(" ")],
      ],
    ),
    "",
    "## The inventory",
    "",
    `${inventory.length} elements. The key is the stable name of an element: where it is, its component, what it says.`,
    "",
    "<details><summary>Every element and the row that stands for it</summary>",
    "",
    table(
      ["Element", "In the mockups", "Says", "Row"],
      inventory.map((e) => [code(e.key), `${e.file}:${e.line}`, e.label, CONTROLS.filter((c) => c.from.includes(e.key)).map((c) => code(c.id)).join(" ") || `not a control: ${NOT_CONTROLS[e.key]?.is ?? "?"}`]),
    ),
    "",
    "</details>",
    "",
  ].join("\n");
}

