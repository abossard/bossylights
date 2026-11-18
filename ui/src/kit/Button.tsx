import { Show } from "solid-js";
import type { Pressable, Tone } from "./common.ts";

export interface ButtonProps extends Pressable {
  label: string;
  /** A second, quieter line. The reason of `off` takes its place. */
  sub?: string;
  tone?: Tone;
  /** Filled: it is on, or it is the one main action. */
  on?: boolean;
  /** A control pressed during a show fires on the way down. An editor button fires on the way up, so sliding off cancels. */
  fires?: "down" | "up";
  onPress: () => void;
}

/** A button. 44 px high, square. Off, it is dashed and says why under its label. */
export function Button(props: ButtonProps) {
  const down = (e: PointerEvent) => {
    if (props.fires === "down" && e.button === 0) props.onPress();
  };
  // A click with no pointer behind it (detail 0) is Enter or Space.
  const click = (e: MouseEvent) => {
    if (props.fires !== "down" || e.detail === 0) props.onPress();
  };
  return (
    <button type="button" class="k-btn" classList={{ "is-on": props.on === true }} data-tone={props.tone ?? "plain"} disabled={props.off !== undefined} onPointerDown={down} onClick={click}>
      <Show when={props.mapped}>
        <i class="k-mapped" aria-hidden="true" />
      </Show>
      <span class="k-label">{props.label}</span>
      <Show when={props.off ?? props.sub}>{(line) => <span class="k-reason">{line()}</span>}</Show>
    </button>
  );
}
