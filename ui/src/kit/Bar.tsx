import { Show } from "solid-js";
import { unbroken, type Tone } from "./common.ts";
import { dragHandlers } from "./dragging.ts";

/**
 * | state    | means                                   |
 * |----------|-----------------------------------------|
 * | plain    | there, a flat fill                      |
 * | selected | picked: a paper frame                   |
 * | dashed   | not now: planned, empty, waiting        |
 * | lifted   | being dragged                           |
 * | clash    | it overlaps another one: a red bar      |
 */
export type BarState = "plain" | "selected" | "dashed" | "lifted" | "clash";

export interface BarProps {
  label: string;
  sub?: string;
  tone?: Tone;
  /** A colour of its own, for things that belong to no row: a group of lights. */
  colour?: string;
  state?: BarState;
  onPress: () => void;
  /** It can be dragged along its line. `dx`: how far, in px. */
  onDrop?: (dx: number) => void;
  onLift?: () => void;
  onCancel?: () => void;
}

/** A flat block on a line: a section of a song, an item of a sequence, a light in the universe grid. */
export function Bar(props: BarProps) {
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
      class="k-bar"
      data-tone={props.tone ?? "plain"}
      data-state={props.state ?? "plain"}
      style={props.colour ? { "--own": props.colour } : {}}
      aria-current={props.state === "selected"}
      onPointerDown={(e) => props.onDrop && drag.onPointerDown(e)}
      onPointerMove={drag.onPointerMove}
      onPointerUp={drag.onPointerUp}
      onPointerCancel={drag.onPointerCancel}
      onKeyDown={drag.onKeyDown}
      onClick={(e) => (e.detail === 0 || !props.onDrop) && props.onPress()}
    >
      <span class="k-label">{unbroken(props.label)}</span>
      <Show when={props.sub}>{(sub) => <span class="k-sub">{sub()}</span>}</Show>
    </button>
  );
}
