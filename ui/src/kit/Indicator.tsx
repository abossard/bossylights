import { Show } from "solid-js";
import type { Pressable } from "./common.ts";

/**
 * | state | lamp                           | means                   |
 * |-------|--------------------------------|-------------------------|
 * | ready | filled mute                    | starts with LIVE        |
 * | on    | filled paper, red if `danger`  | in effect now           |
 * | off   | empty, the word struck through | turned off              |
 */
export type IndicatorState = "ready" | "on" | "off";

export interface IndicatorProps extends Pressable {
  label: string;
  state: IndicatorState;
  /** Red while it is on: recording. */
  danger?: boolean;
  onPress: () => void;
}

const SAYS: Record<IndicatorState, string> = { ready: "ready, starts with LIVE", on: "on", off: "turned off" };

/** A lamp and a word beside the LIVE switch. */
export function Indicator(props: IndicatorProps) {
  return (
    <button
      type="button"
      class="k-ind"
      classList={{ "is-danger": props.danger === true }}
      data-state={props.state}
      aria-label={`${props.label}: ${SAYS[props.state]}`}
      aria-pressed={props.state !== "off"}
      disabled={props.off !== undefined}
      onClick={() => props.onPress()}
    >
      <Show when={props.mapped}>
        <i class="k-mapped" aria-hidden="true" />
      </Show>
      <i class="k-lamp" aria-hidden="true" />
      <span class="k-ind-text">
        <span class="k-ind-word">{props.label}</span>
        <Show when={props.off}>{(reason) => <span class="k-reason">{reason()}</span>}</Show>
      </span>
    </button>
  );
}
