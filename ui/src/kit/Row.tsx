import { Show } from "solid-js";
import type { Pressable, Tone } from "./common.ts";
import { dragHandlers } from "./dragging.ts";

export interface RowProps extends Pressable {
  title: string;
  sub?: string;
  /** The colour of the thing, as a small field at the left. */
  swatch?: string;
  /** One word at the right: its state. */
  mark?: string;
  markTone?: Tone;
  /** `selected`: this one is open or picked. `lifted`: it is being dragged. */
  state?: "plain" | "selected" | "lifted";
  onPress: () => void;
  /** It can be dragged somewhere: called when it is dropped. */
  onDrop?: () => void;
  onLift?: () => void;
  onCancel?: () => void;
}

/** One row of a list that can be picked, and dragged when `onDrop` is given. 44 px high. */
export function Row(props: RowProps) {
  const drag = dragHandlers(() => ({
    // A pointer press comes from the drag machine: after a drop there is no press.
    onPress: props.onPress,
    ...(props.onDrop ? { onDrop: props.onDrop } : {}),
    ...(props.onLift ? { onMove: props.onLift } : {}),
    ...(props.onCancel ? { onCancel: props.onCancel } : {}),
  }));
  return (
    <button
      type="button"
      class="k-row"
      data-state={props.state ?? "plain"}
      aria-current={props.state === "selected"}
      disabled={props.off !== undefined}
      onPointerDown={(e) => props.onDrop && drag.onPointerDown(e)}
      onPointerMove={drag.onPointerMove}
      onPointerUp={drag.onPointerUp}
      onPointerCancel={drag.onPointerCancel}
      onKeyDown={drag.onKeyDown}
      onClick={(e) => (e.detail === 0 || !props.onDrop) && props.onPress()}
    >
      <Show when={props.swatch}>{(colour) => <i class="k-row-swatch" style={{ background: colour() }} aria-hidden="true" />}</Show>
      <span class="k-row-text">
        <span class="k-label">{props.title}</span>
        <Show when={props.off ?? props.sub}>{(line) => <span class={props.off === undefined ? "k-sub" : "k-reason"}>{line()}</span>}</Show>
      </span>
      <Show when={props.mark}>
        {(mark) => (
          <span class="k-mark" data-tone={props.markTone ?? "plain"}>
            {mark()}
          </span>
        )}
      </Show>
    </button>
  );
}
