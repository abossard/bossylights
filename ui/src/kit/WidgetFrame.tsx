import { dragHandlers } from "./dragging.ts";

export interface WidgetFrameProps {
  name: string;
  /** Its size in cells, as it reads: "2 x 2". */
  size: string;
  /** How many px one cell of the grid is. */
  cell: number;
  lifted?: boolean;
  /** Moved by whole cells. */
  onMove: (dx: number, dy: number) => void;
  /** Resized by whole cells. */
  onResize: (dw: number, dh: number) => void;
  onRemove: () => void;
  onLift?: () => void;
  onCancel?: () => void;
}

const ARROWS: Record<string, readonly [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };

/**
 * The frame over a widget while the surface is edited. It covers the widget, so the widget does not play,
 * and says its name and size: that is the reason. Drag moves it, arrows move it, Shift and arrows resize it,
 * Delete removes it.
 */
export function WidgetFrame(props: WidgetFrameProps) {
  const cells = (px: number) => Math.round(px / props.cell);
  const drag = dragHandlers(() => ({
    onDrop: (dx, dy) => props.onMove(cells(dx), cells(dy)),
    ...(props.onLift ? { onMove: props.onLift } : {}),
    ...(props.onCancel ? { onCancel: props.onCancel } : {}),
  }));
  const key = (e: KeyboardEvent) => {
    drag.onKeyDown(e);
    const arrow = ARROWS[e.key];
    if (arrow) {
      e.preventDefault();
      if (e.shiftKey) props.onResize(arrow[0], arrow[1]);
      else props.onMove(arrow[0], arrow[1]);
    }
    if (e.key === "Delete" || e.key === "Backspace") props.onRemove();
  };
  return (
    <div
      class="k-wframe"
      classList={{ "is-lifted": props.lifted === true }}
      role="group"
      tabindex={0}
      aria-label={`${props.name}, ${props.size}. Arrows move, Shift and arrows resize, Delete removes`}
      onPointerDown={drag.onPointerDown}
      onPointerMove={drag.onPointerMove}
      onPointerUp={drag.onPointerUp}
      onPointerCancel={drag.onPointerCancel}
      onKeyDown={key}
    >
      <span class="k-wframe-name">{props.name}</span>
      <span class="k-wframe-size">{props.size}</span>
    </div>
  );
}
