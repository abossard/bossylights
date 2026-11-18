// The frames of a catalog page: the control alone, once per situation, once per state. And the search
// of the index. Calculations over the behaviour table.
import { MACHINES, type Machine } from "../controls/machines.ts";
import { SITUATIONS } from "../controls/situations.ts";
import type { Control } from "../controls/spec.ts";
import { inSituation, inState, usual, type Look } from "./look.ts";

/**
 * | kind      | shows the control                               |
 * |-----------|-------------------------------------------------|
 * | alone     | in the usual state, at real size                |
 * | situation | with one fact true: a column of the table       |
 * | state     | in one of the states it can show                |
 */
export interface Frame {
  key: string;
  kind: "alone" | "situation" | "state";
  title: string;
  /** What is true in this frame. */
  fact: string;
  /** What happens here, or the reason it is off or gone: the cell of the table. */
  says: string;
  is: "works" | "off" | "gone";
  look: Look;
}

const USUAL_STATE = "The usual state: LIVE off, engine connected, lights patched, nothing held, no song, autopilot off.";

const standing = (look: Look): Frame["is"] => (look.gone ? "gone" : look.off && !look.offOption ? "off" : "works");

/** Every frame of a control's page, in the order they are shown. */
export function framesOf(control: Control): Frame[] {
  // Alone, it is drawn where it exists. If the usual state switches it off, it says why here too.
  const first = control.cells["live-off"];
  const alone = first.is === "off" ? { ...usual(control), off: first.says, offOption: "" } : usual(control);
  const machine = control.machine ? (MACHINES[control.machine] as Machine<string, string>) : undefined;
  return [
    { key: "alone", kind: "alone", title: "Alone", fact: USUAL_STATE, says: control.does, is: standing(alone), look: alone },
    ...SITUATIONS.map((s): Frame => {
      const cell = control.cells[s.id];
      return { key: s.id, kind: "situation", title: s.name, fact: s.fact, says: cell.says, is: cell.is, look: inSituation(control, s.id) };
    }),
    ...control.states.map((state): Frame => {
      const look = inState(control, state);
      const says = look.gone || look.off || (machine?.states[look.at] ?? "");
      return { key: `state:${state}`, kind: "state", title: state, fact: machine ? `${machine.title}: ${look.at}` : "", says, is: standing(look), look };
    }),
  ];
}

/** What the search reads of a control. */
const words = (c: Control) => [c.name, c.id, c.screen, c.part, c.kit, c.kind, c.first.verdict, c.does, ...c.sends].join(" ").toLowerCase();

/** The controls that have every word of the query, in the order of the table. */
export function search(controls: readonly Control[], query: string): Control[] {
  const wanted = query.toLowerCase().split(/\s+/).filter(Boolean);
  return controls.filter((c) => wanted.every((w) => words(c).includes(w)));
}

/** The screens that have controls, in the order of the table. A screen is a tag of a control, not its place in the index. */
export const screensOf = (controls: readonly Control[]): string[] => [...new Set(controls.map((c) => c.screen))];
