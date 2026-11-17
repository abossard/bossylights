import type { Layer } from "../fake/data";

export interface XYPadProps {
  /** 0..100, left to right */
  x: number;
  /** 0..100, bottom to top */
  y: number;
  onChange: (x: number, y: number) => void;
  label: string;
  layer?: Layer;
  class?: string;
}

/** Two values at once: drag anywhere, or arrow keys (Shift for steps of 10). */
export function XYPad(props: XYPadProps) {
  let box!: HTMLDivElement;

  const fromPointer = (e: PointerEvent) => {
    const r = box.getBoundingClientRect();
    const clamp = (v: number) => Math.round(Math.max(0, Math.min(1, v)) * 100);
    props.onChange(clamp((e.clientX - r.left) / r.width), clamp((r.bottom - e.clientY) / r.height));
  };

  const onKey = (e: KeyboardEvent) => {
    const s = e.shiftKey ? 10 : 1;
    const d: Record<string, [number, number]> = {
      ArrowLeft: [-s, 0],
      ArrowRight: [s, 0],
      ArrowUp: [0, s],
      ArrowDown: [0, -s],
    };
    if (!d[e.key]) return;
    e.preventDefault();
    const c = (v: number) => Math.max(0, Math.min(100, v));
    props.onChange(c(props.x + d[e.key][0]), c(props.y + d[e.key][1]));
  };

  return (
    <div
      ref={box}
      class={`xy ${props.layer ? `layer-${props.layer}` : ""} ${props.class ?? ""}`}
      role="slider"
      tabIndex={0}
      aria-label={props.label}
      aria-valuetext={`${props.x}, ${props.y}`}
      aria-valuenow={props.x}
      onPointerDown={(e) => {
        box.setPointerCapture(e.pointerId);
        fromPointer(e);
      }}
      onPointerMove={(e) => box.hasPointerCapture(e.pointerId) && fromPointer(e)}
      onKeyDown={onKey}
      style={{ "--x": `${props.x}%`, "--y": `${100 - props.y}%` }}
    >
      <i class="xy-grid" />
      <i class="xy-h" />
      <i class="xy-v" />
      <i class="xy-dot" />
    </div>
  );
}
