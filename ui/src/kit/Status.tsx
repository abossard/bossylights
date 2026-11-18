import { Show } from "solid-js";
import type { Signal } from "./common.ts";

/**
 * | kind  | is                                   | used for                                      |
 * |-------|--------------------------------------|-----------------------------------------------|
 * | lamp  | a round lamp and a word              | DJ link, Recording, the engine in the menu bar |
 * | pill  | a word in a box                      | the state of a row, what runs in a lane       |
 * | flag  | a paper flag in the display face     | NOW                                           |
 * | value | a reading in the display face        | a level, a count of beats, an address         |
 */
export type StatusKind = "lamp" | "pill" | "flag" | "value";

/**
 * | fill    | means                               | lamp           | pill           |
 * |---------|-------------------------------------|----------------|----------------|
 * | solid   | it is so, now                       | lit            | filled         |
 * | outline | it is so, but not in effect         | empty          | outlined       |
 * | dashed  | not now: waiting, off               | empty, struck  | dashed outline |
 */
export type StatusFill = "solid" | "outline" | "dashed";

export interface StatusProps {
  kind: StatusKind;
  /** What it says: a state in a word, a value as it reads. */
  label: string;
  /** What it is the status of, where its place does not say: "Manual", "Fade". */
  name?: string;
  colour?: Signal;
  fill?: StatusFill;
}

/** Says what is so. Read only: it is never pressed. What is pressed around it is a Button or an Item. */
export function Status(props: StatusProps) {
  return (
    <span class="k-status" data-kind={props.kind} data-colour={props.colour ?? "plain"} data-fill={props.fill ?? "solid"}>
      <Show when={props.name}>{(name) => <span class="k-status-name">{name()}</span>}</Show>
      <Show when={props.kind === "lamp"}>
        <i class="k-lamp" aria-hidden="true" />
      </Show>
      <span class="k-status-label">{props.label}</span>
    </span>
  );
}
