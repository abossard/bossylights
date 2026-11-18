import { Show } from "solid-js";
import type { Pressable } from "./common.ts";

export interface LiveSwitchProps extends Pressable {
  on: boolean;
  onToggle: (next: boolean) => void;
}

/** LIVE: a paper slab with the knob right while the night runs, an outline with the knob left while preparing. */
export function LiveSwitch(props: LiveSwitchProps) {
  return (
    <button
      type="button"
      class="k-live"
      role="switch"
      aria-checked={props.on}
      aria-label={props.on ? "LIVE is on: the night is running" : "LIVE is off: preparing"}
      disabled={props.off !== undefined}
      onClick={() => props.onToggle(!props.on)}
    >
      <Show when={props.mapped}>
        <i class="k-mapped" aria-hidden="true" />
      </Show>
      <span class="k-live-word">LIVE {props.on ? "on" : "off"}</span>
      <i class="k-live-knob" aria-hidden="true" />
    </button>
  );
}
