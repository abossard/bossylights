// What is wrong with the behaviour table. A calculation: the table and the inventory in, a list of faults out.
import { COMMANDS, type CommandName } from "./commands.ts";
import type { Contradiction } from "./findings.ts";
import { MACHINES, type Machine } from "./machines.ts";
import { GESTURES, SITUATIONS } from "./situations.ts";
import type { Control } from "./spec.ts";
import type { NotAControl } from "./table.ts";

/** The longest reason that still fits under a label. */
const REASON = 40;

/** The faults of the rows on their own: names, cells, gestures, what they send. */
export function rowFaults(controls: readonly Control[]): string[] {
  const seen = new Set<string>();
  return controls.flatMap((c) => {
    const faults: string[] = [];
    if (seen.has(c.id)) faults.push(`${c.id}: the id is used twice`);
    seen.add(c.id);
    if (!/^[a-z]+(\.[a-z-]+)+$/.test(c.id)) faults.push(`${c.id}: not a stable name (screen.part.control)`);
    if (!c.from.length && !c.decided) faults.push(`${c.id}: stands for no element of the mockups, and names no issue that decided it`);
    if (!c.sends.length && !c.ui) faults.push(`${c.id}: sends no command and names no state of the screen`);
    // Playback decides what plays and who wins. A control that plays or sets only sends: it keeps no state of its own.
    if ((c.kind === "play" || c.kind === "setting") && c.ui) faults.push(`${c.id}: plays or sets, and keeps a state of the screen (${c.ui}). It sends a command, the engine decides`);
    for (const text of [c.name, c.does, c.part, c.feedback.screen, c.feedback.controller, c.undo.why, c.learn.why, c.first.why]) {
      if (!text.trim()) faults.push(`${c.id}: an answer is empty`);
    }
    for (const g of GESTURES) if (!c.gestures[g].trim()) faults.push(`${c.id}: no answer for the gesture "${g}"`);
    for (const s of SITUATIONS) {
      const cell = c.cells[s.id];
      if (!cell.says.trim()) faults.push(`${c.id}: the cell "${s.name}" is empty`);
      if (cell.is === "off" && cell.says.length > REASON) faults.push(`${c.id}: the reason in "${s.name}" does not fit under a label (${cell.says.length} letters)`);
    }
    return faults;
  });
}

/** The faults between the table and the inventory: an element nobody stands for, a key that no longer exists. */
export function inventoryFaults(controls: readonly Control[], notControls: Record<string, NotAControl>, keys: readonly string[]): string[] {
  const known = new Set(keys);
  const claimed = new Set([...controls.flatMap((c) => c.from), ...Object.keys(notControls)]);
  return [
    ...keys.filter((k) => !claimed.has(k)).map((k) => `${k}: an element of the mockups with no row and no reason`),
    ...[...claimed].filter((k) => !known.has(k)).map((k) => `${k}: named in the table, not in the inventory`),
    ...controls.flatMap((c) => c.from.filter((k) => k in notControls).map((k) => `${k}: a row (${c.id}) and "not a control" at once`)),
  ];
}

const sentBy = (machine: Machine<string, string>): CommandName[] =>
  Object.values(machine.table).flatMap((row) => Object.values(row).flatMap((step) => [...(step.sends ?? [])]));

/** Commands nobody sends. The list is the union of what the controls send, plus what the engine has today. */
export function unusedCommands(controls: readonly Control[]): string[] {
  const sent = new Set<string>([...controls.flatMap((c) => [...c.sends]), ...Object.values(MACHINES).flatMap((m) => sentBy(m as Machine<string, string>))]);
  return Object.entries(COMMANDS)
    .filter(([name, spec]) => !sent.has(name) && !("engine" in spec))
    .map(([name]) => `${name}: on the list, sent by no control`);
}

/** States of a machine that cannot be reached from its start, and machines no row uses. */
export function machineFaults(controls: readonly Control[]): string[] {
  const used = new Set(controls.map((c) => c.machine));
  return Object.entries(MACHINES).flatMap(([name, m]) => {
    const machine = m as Machine<string, string>;
    const reached = new Set([machine.start]);
    for (let grew = true; grew; ) {
      grew = false;
      for (const state of [...reached]) {
        for (const step of Object.values(machine.table[state] ?? {})) {
          if (!reached.has(step.to)) grew = reached.add(step.to).size > 0;
        }
      }
    }
    return [
      ...Object.keys(machine.states).filter((s) => !reached.has(s)).map((s) => `${name}: the state "${s}" is never reached`),
      ...(used.has(name as keyof typeof MACHINES) ? [] : [`${name}: no row uses this machine`]),
    ];
  });
}

/** Contradictions that point at a row that does not exist. */
export function findingFaults(controls: readonly Control[], contradictions: readonly Contradiction[]): string[] {
  const ids = new Set(controls.map((c) => c.id));
  return contradictions.flatMap((c) => c.where.filter((id) => !ids.has(id)).map((id) => `"${c.what}" points at ${id}, which is not a row`));
}
