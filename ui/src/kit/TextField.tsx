import { Show } from "solid-js";
import type { Pressable } from "./common.ts";

export interface TextFieldProps extends Pressable {
  label: string;
  value: string;
  placeholder?: string;
  /** Every letter counts at once: a search. Otherwise Enter saves and Escape puts the old text back. */
  live?: boolean;
  onText: (text: string) => void;
}

/** A line of text, 44 px high, with its label above. */
export function TextField(props: TextFieldProps) {
  const key = (e: KeyboardEvent & { currentTarget: HTMLInputElement }) => {
    if (props.live) return;
    if (e.key === "Enter") props.onText(e.currentTarget.value);
    if (e.key === "Escape") e.currentTarget.value = props.value;
  };
  return (
    <label class="k-field">
      <span class="k-field-label">{props.label}</span>
      <input
        class="k-input"
        type="text"
        value={props.value}
        placeholder={props.placeholder ?? ""}
        disabled={props.off !== undefined}
        onInput={(e) => props.live && props.onText(e.currentTarget.value)}
        onKeyDown={key}
      />
      <Show when={props.off}>{(reason) => <span class="k-reason">{reason()}</span>}</Show>
    </label>
  );
}
