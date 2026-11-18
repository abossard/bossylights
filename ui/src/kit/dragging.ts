// One drag for every component that can be dragged: the `drag` machine of the behaviour table, fed by a pointer.
// Below 8 px it was a press. Escape puts it back. Nothing is sent before the finger lifts.
import { drag, step } from "../controls/machines.ts";

/** A drag needs this much movement, not a wait. */
export const DRAG_STARTS_AT = 8;

type DragState = keyof typeof drag.states;

export interface DragEnds {
  /** It came up before it moved 8 px: a press. */
  onPress?: () => void;
  /** It moves. `dx`, `dy`: how far from where it went down, in px. */
  onMove?: (dx: number, dy: number) => void;
  /** It was dropped. */
  onDrop?: (dx: number, dy: number) => void;
  /** Escape, or the pointer was lost: it goes back. */
  onCancel?: () => void;
}

/** Keeps the pointer on the element it went down on, so a drag goes on outside it. */
export function capture(target: Element, pointer: number) {
  try {
    target.setPointerCapture(pointer);
  } catch {
    // the pointer is already gone: the press still counts
  }
}

/** What a transition of the drag machine means for the one who listens. A calculation. */
export function dragOutcome(from: DragState, to: DragState, event: keyof typeof drag.events): "press" | "drop" | "cancel" | "move" | "none" {
  if (from === "armed" && to === "idle" && event === "up") return "press";
  if (from === "dragging" && to === "idle" && event === "up") return "drop";
  if (from !== "idle" && to === "idle" && event === "cancel") return "cancel";
  if (to === "dragging") return "move";
  return "none";
}

/** Pointer and key handlers for one draggable element. The only state is the machine's and where it went down. */
export function dragHandlers(ends: () => DragEnds) {
  let state: DragState = drag.start;
  let x0 = 0;
  let y0 = 0;
  let dx = 0;
  let dy = 0;

  const send = (event: keyof typeof drag.events) => {
    const from = state;
    state = step(drag, from, event).to;
    const outcome = dragOutcome(from, state, event);
    if (outcome === "press") ends().onPress?.();
    if (outcome === "drop") ends().onDrop?.(dx, dy);
    if (outcome === "cancel") ends().onCancel?.();
    if (outcome === "move") ends().onMove?.(dx, dy);
  };

  return {
    onPointerDown: (e: PointerEvent) => {
      if (e.button > 0) return;
      if (e.currentTarget instanceof Element) capture(e.currentTarget, e.pointerId);
      x0 = e.clientX;
      y0 = e.clientY;
      dx = 0;
      dy = 0;
      send("down");
    },
    onPointerMove: (e: PointerEvent) => {
      if (state === "idle") return;
      dx = e.clientX - x0;
      dy = e.clientY - y0;
      if (state === "dragging" || Math.hypot(dx, dy) >= DRAG_STARTS_AT) send("move");
    },
    onPointerUp: () => {
      send("up");
    },
    onPointerCancel: () => {
      send("cancel");
    },
    onKeyDown: (e: KeyboardEvent) => {
      if (e.key === "Escape") send("cancel");
    },
  };
}
