// The kit as a document: docs/design/kit.md, written from ui/src/controls and ui/src/catalog/compose.ts (issue #102).
// Which primitive draws which control, with which props. A calculation: the data in, Markdown out.
import { byFunction, recipe } from "../ui/src/catalog/compose.ts";
import { PRIMITIVE_FOR, PRIMITIVES } from "../ui/src/controls/spec.ts";
import { CONTROLS } from "../ui/src/controls/table.ts";

const cellText = (text) => String(text).replace(/\|/g, "\\|").replace(/\n/g, " ");
const row = (cells) => `| ${cells.map(cellText).join(" | ")} |`;
const table = (head, rows) => [row(head), row(head.map(() => "---")), ...rows.map(row)].join("\n");
const code = (text) => `\`${text}\``;

const KIT = Object.keys(PRIMITIVES);
const COLOURS = [
  ["plain", "nothing special", "an outline, paper when on"],
  ["paper", "now, or the one main action", "`--paper`"],
  ["manual", "your hands", "`--layer-manual`"],
  ["show", "the show", "`--layer-show`"],
  ["autopilot", "the autopilot", "`--layer-autopilot`"],
  ["danger", "blackout, recording, trouble", "`--danger`"],
  ["mute, ok, warn", "a lamp only: ready, connected, look here", "`--mute`, `--ok`, `--warn`"],
];

/** The fixed props of a control, as they are written on its primitive. */
const propsOf = (control) => Object.entries(recipe(control).props).map(([name, value]) => (value === "yes" ? name : `${name}: ${value}`)).join(", ");

export function kitDoc() {
  const groups = byFunction(CONTROLS, KIT);
  const drawnWith = (primitive) => CONTROLS.filter((c) => recipe(c).primitive === primitive);
  const carried = (primitive) => CONTROLS.filter((c) => recipe(c).inside.some((part) => part.startsWith(primitive))).length;
  const kinds = groups.length;

  return [
    "# The kit: ten primitives, and what every control is made of",
    "",
    "Written by `node scripts/write-kit-doc.mjs` from `ui/src/controls` and `ui/src/catalog/compose.ts`. Change the data there, never this file.",
    "The catalog shows the same as live pages, primitives first: `npm run catalog`.",
    "",
    "```mermaid",
    "flowchart LR",
    `  R["${CONTROLS.length} controls<br/>one row each"] --> K["${KIT.length} primitives<br/>named by what they are for"]`,
    `  K --> V["${kinds} kinds in use"]`,
    '  K --> C["colour<br/>one prop on all of them"]',
    '  R -. "where it lives" .-> T["a tag:<br/>the screen"]',
    "```",
    "",
    table(["Rule", ""], [
      ["A primitive is named by what it is for", "never by where it is used"],
      ["A control is a primitive with props", "or a few together: a stepper is two Buttons and a Status"],
      ["Colour is a prop", "the same six on every primitive. Row colours only say whose a thing is"],
      ["The behaviour table stays the truth", "`kit` on a row names the primitive and the kind. What the control does, sends and shows is the row"],
    ]),
    "",
    "## The primitives",
    "",
    table(
      ["Primitive", "Is for", "Kinds", "Controls", "Carried inside"],
      KIT.map((p) => [`**${p}**`, PRIMITIVE_FOR[p], PRIMITIVES[p].join(", ") || "one", drawnWith(p).length || "–", carried(p) || "–"]),
    ),
    "",
    "## Colour",
    "",
    table(["Colour", "Means", "Token"], COLOURS),
    "",
    "## Every control, by primitive and kind",
    "",
    "Fixed props are the ones that never change for the control. Its words, its state and its value come from the row.",
    "",
    ...groups.flatMap((group) => [
      `### ${group.primitive}${group.kind ? `, ${group.kind}` : ""} (${group.controls.length})`,
      "",
      table(
        ["Control", "Name", "Where", "Colour", "Fixed props", "Carries"],
        group.controls.map((c) => [c.name, code(c.id), c.screen, recipe(c).colour, propsOf(c), recipe(c).inside.join(" + ")]),
      ),
      "",
    ]),
  ].join("\n");
}
