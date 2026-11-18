import { Show } from "solid-js";
import type { Pressable, Tone } from "./common.ts";

export interface ToggleProps extends Pressable {
  label: string;
  on: boolean;
  /** A second, quieter line: "Waits for LIVE". The reason of `off` takes its place. */
  sub?: string;
  /** The fill while it is on. Paper unless it belongs to a row or is red. */
  tone?: Tone;
  /** What it will be after the press. */
  onToggle: (next: boolean) => void;
}

/** A button that stays on until it is pressed again. On is a fill, off an outline. */
export function Toggle(props: ToggleProps) {
  return (
    <button
      type="button"
      class="k-btn"
      classList={{ "is-on": props.on }}
      data-tone={props.tone ?? "paper"}
      aria-pressed={props.on}
      disabled={props.off !== undefined}
      onClick={() => props.onToggle(!props.on)}
    >
      <Show when={props.mapped}>
        <i class="k-mapped" aria-hidden="true" />
      </Show>
      <span class="k-label">{props.label}</span>
      <Show when={props.off ?? props.sub}>{(line) => <span class="k-reason">{line()}</span>}</Show>
    </button>
  );
}
