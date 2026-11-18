import { onCleanup, onMount } from "solid-js";

export interface PreviewProps {
  /** `matrix`: a grid of pixels, as a pixel panel shows an effect. */
  kind?: "matrix";
  /** What it shows, for screen readers: "Plasma on the panel". */
  label: string;
  columns: number;
  rows: number;
  /** The colour of one pixel at one time. A calculation: the same place and time give the same colour. */
  paint: (column: number, row: number, seconds: number) => string;
  /** One frame that does not move: for many side by side. Also what people get who asked their system for less motion. */
  still?: boolean;
  /** `small`: inside an option of a picker. */
  size?: "small" | "regular";
}

/** The frame a still preview shows: far enough in for an effect to have a shape. */
const STILL_AT = 0.6;

/**
 * See it before it plays: an effect, running on a small model of the thing it will play on. Read only.
 * It never touches the lights and takes no gesture. What is on the lights is shown by a Status, not by this.
 */
export function Preview(props: PreviewProps) {
  let canvas: HTMLCanvasElement | undefined;
  onMount(() => {
    const surface = canvas;
    const context = surface?.getContext("2d");
    if (!surface || !context) return;
    const still = props.still === true || window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const draw = (seconds: number) => {
      const cell = surface.width / props.columns;
      context.clearRect(0, 0, surface.width, surface.height);
      for (let row = 0; row < props.rows; row += 1) {
        for (let column = 0; column < props.columns; column += 1) {
          context.fillStyle = props.paint(column, row, seconds);
          context.fillRect(column * cell + 1, row * cell + 1, cell - 2, cell - 2);
        }
      }
    };
    if (still) return draw(STILL_AT);
    const start = performance.now();
    let frame = requestAnimationFrame(function next(now) {
      draw((now - start) / 1000);
      frame = requestAnimationFrame(next);
    });
    onCleanup(() => cancelAnimationFrame(frame));
  });
  const cell = () => (props.size === "small" ? 6 : 12);
  return <canvas ref={(el) => (canvas = el)} class="k-preview" data-kind={props.kind ?? "matrix"} role="img" aria-label={props.label} width={props.columns * cell() * 2} height={props.rows * cell() * 2} style={{ width: `${props.columns * cell()}px`, height: `${props.rows * cell()}px` }} />;
}
