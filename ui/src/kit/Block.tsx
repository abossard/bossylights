import { Show } from "solid-js";
import { unbroken, type Colour, type Standing } from "./common.ts";
import { dragHandlers } from "./dragging.ts";

const ARROWS: Record<string, readonly [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };

export interface BlockProps {
  /**
   * | kind  | is                                               | arranged by                                   |
   * |-------|--------------------------------------------------|-----------------------------------------------|
   * | bar   | a flat block on a line: a section, a step, a light | a drag along the line. A press picks it      |
   * | frame | the frame over a widget while the surface is edited | a drag, arrows, Shift and arrows, Delete     |
   */
  kind: "bar" | "frame";
  label: string;
  sub?: string;
  /** Whose it is: the fill of a bar. */
  colour?: Colour;
  /** A colour of its own, for things that belong to no row: a group of lights. */
  own?: string;
  standing?: Standing;
  /** A press picks it. Leave it out for a frame: a frame is only arranged. */
  onPress?: () => void;
  /** It was dropped. How far it went, in px. */
  onDrop?: (dx: number, dy: number) => void;
  onLift?: () => void;
  onCancel?: () => void;
  /** An arrow key: one step. */
  onNudge?: (dx: number, dy: number) => void;
  /** Shift and an arrow key: one step wider, narrower, taller, shorter. */
  onResize?: (dw: number, dh: number) => void;
  /** Delete or Backspace. */
  onRemove?: () => void;
}

/** A thing placed in time or on a grid. It is arranged: dragged, nudged, resized. Nothing is sent before it is dropped. */
export function Block(props: BlockProps) {
  const drag = dragHandlers(() => ({
    // A pointer press comes from the drag machine: after a drop there is no press.
    ...(props.onPress ? { onPress: props.onPress } : {}),
    ...(props.onDrop ? { onDrop: props.onDrop } : {}),
    ...(props.onLift ? { onMove: props.onLift } : {}),
    ...(props.onCancel ? { onCancel: props.onCancel } : {}),
  }));
  const key = (e: KeyboardEvent) => {
    drag.onKeyDown(e);
    const arrow = ARROWS[e.key];
    const move = e.shiftKey ? props.onResize : props.onNudge;
    if (arrow && move) {
      e.preventDefault();
      move(arrow[0], arrow[1]);
    }
    if (e.key === "Delete" || e.key === "Backspace") props.onRemove?.();
  };
  const shared = () => ({
    "data-kind": props.kind,
    "data-colour": props.colour ?? "plain",
    "data-standing": props.standing ?? "plain",
    style: props.own === undefined ? {} : { "--own": props.own },
    onPointerMove: drag.onPointerMove,
    onPointerUp: drag.onPointerUp,
    onPointerCancel: drag.onPointerCancel,
    onKeyDown: key,
  });

  return (
    <Show
      when={props.kind === "bar"}
      fallback={
        <div class="k-block" {...shared()} role="group" tabindex={0} aria-label={`${props.label}${props.sub ? `, ${props.sub}` : ""}. Arrows move, Shift and arrows resize, Delete removes`} onPointerDown={drag.onPointerDown}>
          <span class="k-block-name">{props.label}</span>
          <Show when={props.sub}>{(sub) => <span class="k-sub">{sub()}</span>}</Show>
        </div>
      }
    >
      <button
        type="button"
        class="k-block"
        {...shared()}
        aria-current={props.standing === "picked"}
        onPointerDown={(e) => props.onDrop && drag.onPointerDown(e)}
        onClick={(e) => (e.detail === 0 || !props.onDrop) && props.onPress?.()}
      >
        <span class="k-label">{unbroken(props.label)}</span>
        <Show when={props.sub}>{(sub) => <span class="k-sub">{sub()}</span>}</Show>
      </button>
    </Show>
  );
}
