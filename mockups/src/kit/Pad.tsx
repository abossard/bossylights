import { For, Show } from "solid-js";

export type PadState = "idle" | "autopilot" | "held" | "flash";

export interface PadProps {
  label: string;
  /** Colour chip. Several colours draw a striped chip. */
  colors: string[];
  state: PadState;
  /** Small line under the label. Defaults to who runs it. */
  sub?: string;
  /** Finger or mouse down. Fires at once, before any hold is known. */
  onDown: () => void;
  /** Finger up. heldMs is how long it was down (0 for keyboard). */
  onUp: (heldMs: number) => void;
  disabled?: boolean;
}

const SUB: Record<PadState, string | undefined> = {
  idle: undefined,
  autopilot: "Autopilot",
  held: "Held live",
  flash: "Flash",
};

/**
 * A look pad. Press fires on down so it feels instant. The screen decides
 * what a short press and a long hold mean, from onDown and onUp(heldMs).
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

  const running = () => props.state !== "idle";

  return (
    <button
      type="button"
      class={`pad pad-${props.state}`}
      aria-pressed={running()}
      disabled={props.disabled}
      onPointerDown={pointerDown}
      onPointerUp={pointerUp}
      onPointerCancel={pointerUp}
      onClick={click}
      onContextMenu={(e) => e.preventDefault()}
    >
      <Show when={running()}>
        <span class="now-flag pad-flag">Now</span>
      </Show>
      <i class="pad-chip" classList={{ "pad-chip-dark": props.colors[0] === "#000000" }}>
        <For each={props.colors}>{(c) => <span style={{ background: c }} />}</For>
      </i>
      <span class="pad-label">{props.label}</span>
      <Show when={props.sub ?? SUB[props.state]}>
        <em class="pad-sub">{props.sub ?? SUB[props.state]}</em>
      </Show>
    </button>
  );
}
