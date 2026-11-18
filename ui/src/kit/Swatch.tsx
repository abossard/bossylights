import { Show } from "solid-js";
import type { Pressable } from "./common.ts";

export interface SwatchProps extends Pressable {
  /** What a press does, for screen readers: "Take red out", "Add amber". */
  label: string;
  colour: string;
  /** `add`: a colour to pick. `remove`: a colour that is set, a press takes it out. */
  does: "add" | "remove";
  onPress: () => void;
}

/** One colour, 44 px square. A colour that is set carries a cross: a press takes it out. */
export function Swatch(props: SwatchProps) {
  return (
    <span class="k-swatch-wrap">
      <button
        type="button"
        class="k-swatch"
        data-does={props.does}
        style={{ "--own": props.colour }}
        aria-label={props.label}
        disabled={props.off !== undefined}
        onClick={() => props.onPress()}
      >
        <Show when={props.does === "remove"}>
          <span aria-hidden="true">×</span>
        </Show>
      </button>
      <Show when={props.off}>{(reason) => <span class="k-reason">{reason()}</span>}</Show>
    </span>
  );
}
