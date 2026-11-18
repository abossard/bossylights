import { For, Show } from "solid-js";
import type { Pressable } from "./common.ts";

export interface SelectProps extends Pressable {
  label: string;
  options: readonly string[];
  value: string;
  onPick: (option: string) => void;
}

/** A list that opens: the system's own, 44 px high, with its label above. */
export function Select(props: SelectProps) {
  return (
    <label class="k-field">
      <span class="k-field-label">{props.label}</span>
      <select class="k-select" disabled={props.off !== undefined} onChange={(e) => props.onPick(e.currentTarget.value)}>
        <For each={props.options}>{(option) => <option selected={option === props.value}>{option}</option>}</For>
      </select>
      <Show when={props.off}>{(reason) => <span class="k-reason">{reason()}</span>}</Show>
    </label>
  );
}
