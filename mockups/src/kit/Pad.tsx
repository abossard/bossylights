import { For, Show } from "solid-js";

/** idle, running (autopilot, show, held by hand), flash: a finger is down, or kept: the pad is off but what it touched is still held until Release. */
export type PadState = "idle" | "autopilot" | "show" | "held" | "flash" | "kept";

export interface PadProps {
  label: string;
  /** The look's colour, drawn as a field across the top. Several colours draw stripes. */
  colors: string[];
  state: PadState;
  /** Finger or mouse down. Fires at once, before any hold is known. */
  onDown: () => void;
  /** Finger up. heldMs is how long it was down (0 for keyboard). */
  onUp: (heldMs: number) => void;
  disabled?: boolean;
}

/** Who runs it, in words: for screen readers and, on tall pads, on the pad itself. */
const RUNS: Record<PadState, string | undefined> = {
  idle: undefined,
  autopilot: "Autopilot",
  show: "Show",
  held: "Manual",
  flash: "Flash",
  kept: undefined,
};

/**
 * A look pad. Its colour is a field, its name is readable, and when it runs
 * the whole pad takes the colour of who runs it (manual, show or autopilot)
 * and carries the NOW flag. A pad that is off but still held keeps a pink mark. Press fires on down so it feels instant; the screen decides what
 * a short press and a long hold mean, from onDown and onUp(heldMs).
 */
export function Pad(props: PadProps) {
  let downAt = 0;
  let down = false;

  const pointerDown = (e: PointerEvent) => {
    if (props.disabled || e.button > 0) return;
    try {
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    } catch {
      // pointer already gone: the press still counts
    }
    down = true;
    downAt = performance.now();
    props.onDown();
  };
  const pointerUp = () => {
    if (!down) return;
    down = false;
    props.onUp(performance.now() - downAt);
  };
  // Keyboard (Enter): click with detail 0 is a tap.
  const click = (e: MouseEvent) => {
    if (e.detail !== 0 || props.disabled) return;
    props.onDown();
    props.onUp(0);
  };

  const running = () => props.state !== "idle" && props.state !== "kept";
  // "Chase L-R" must not break at its hyphen.
  const name = () => props.label.replace(/-/g, "‑");

  return (
    <button
      type="button"
      class={`pad pad-${props.state}`}
      aria-pressed={running()}
      aria-label={running() ? `${props.label}, running: ${RUNS[props.state]}` : props.state === "kept" ? `${props.label}, off, still held until Release` : props.label}
      disabled={props.disabled}
      onPointerDown={pointerDown}
      onPointerUp={pointerUp}
      onPointerCancel={pointerUp}
      onClick={click}
      onContextMenu={(e) => e.preventDefault()}
    >
      <span class="pad-field" classList={{ "is-dark": props.colors[0] === "#000000" }} aria-hidden="true">
        <For each={props.colors}>{(c) => <i style={{ background: c }} />}</For>
      </span>
      <Show when={running()}>
        <span class="now-flag pad-flag" aria-hidden="true">Now</span>
      </Show>
      <Show when={props.state === "kept"}>
        <span class="pad-kept-mark" aria-hidden="true">held</span>
      </Show>
      <span class="pad-name" aria-hidden="true">{name()}</span>
      <Show when={running()}>
        <span class="pad-runs" aria-hidden="true">{RUNS[props.state]}</span>
      </Show>
    </button>
  );
}
