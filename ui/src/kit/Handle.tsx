import { Show } from "solid-js";
import type { Pressable } from "./common.ts";
import { dragHandlers } from "./dragging.ts";

export interface HandleProps extends Pressable {
  /** `edge`: a line moved sideways. `corner`: a corner that resizes. */
  kind: "edge" | "corner";
  /** What it moves, for screen readers: "Edge between Intro and Drop". */
  label: string;
  /** It was dropped. How far it went, in px. */
  onDrop: (dx: number, dy: number) => void;
  /** An arrow key: one step back or forward. */
  onNudge: (by: -1 | 1) => void;
  onLift?: () => void;
  onCancel?: () => void;
}

/** A grip that is only dragged: the edge between two sections, the end of a bar, the corner of a widget. Off, its reason stands beside it. */
export function Handle(props: HandleProps) {
  const off = () => props.off !== undefined;
  const drag = dragHandlers(() => ({
    onDrop: props.onDrop,
    ...(props.onLift ? { onMove: props.onLift } : {}),
    ...(props.onCancel ? { onCancel: props.onCancel } : {}),
  }));
  const key = (e: KeyboardEvent) => {
    if (off()) return;
    drag.onKeyDown(e);
    const by = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : undefined;
    if (by === undefined) return;
    e.preventDefault();
    props.onNudge(by);
  };
  return (
    <>
      <div
        class="k-handle"
        data-kind={props.kind}
        role="separator"
        tabindex={off() ? undefined : 0}
        aria-disabled={off()}
        aria-label={off() ? `${props.label}. ${props.off}` : `${props.label}. Drag, or use the arrows`}
        onPointerDown={(e) => off() || drag.onPointerDown(e)}
        onPointerMove={drag.onPointerMove}
        onPointerUp={drag.onPointerUp}
        onPointerCancel={drag.onPointerCancel}
        onKeyDown={key}
      />
      <Show when={props.off}>{(reason) => <span class="k-reason">{reason()}</span>}</Show>
    </>
  );
}
