import { createUniqueId, For, Show } from "solid-js";
import { unbroken, type Pressable } from "./common.ts";
import { capture } from "./dragging.ts";
import { Status } from "./Status.tsx";

/**
 * | state     | means                                                        |
 * |-----------|--------------------------------------------------------------|
 * | idle      | nobody runs its look                                         |
 * | manual    | it runs by hand, latched                                     |
 * | flash     | a finger is down on it                                       |
 * | kept      | the pad is off, what it touched is still held until Release  |
 * | show      | the show runs its look                                       |
 * | autopilot | the autopilot runs its look                                  |
 */
export type PadState = "idle" | "manual" | "flash" | "kept" | "show" | "autopilot";

export interface PadProps extends Pressable {
  label: string;
  /** The look's colour, a field across the top. Several colours draw stripes. */
  colours: readonly string[];
  state: PadState;
  /** Finger or mouse down. Fires at once, before any hold is known. */
  onDown: () => void;
  /** It came up. `heldMs`: how long it was down, 0 from the keyboard. */
  onUp: (heldMs: number) => void;
  /** The system took the touch while it was down (a palm, a scroll): it never came up as a press. */
  onLost: () => void;
}

/** Who runs it, in words: on tall pads and for screen readers. */
const RUNS: Record<PadState, string> = { idle: "", manual: "Manual", flash: "Flash", kept: "", show: "Show", autopilot: "Autopilot" };

/**
 * A look pad, the primary control. It fires on the way down so it feels instant. What a short press
 * and a long hold mean is decided by the one who listens, from `onDown` and `onUp(heldMs)`, or `onLost`.
 */
export function Pad(props: PadProps) {
  const reason = createUniqueId();
  let downAt = 0;
  let down = false;
  const running = () => RUNS[props.state] !== "";

  const pointerDown = (e: PointerEvent) => {
    if (e.button > 0) return;
    if (e.currentTarget instanceof Element) capture(e.currentTarget, e.pointerId);
    down = true;
    downAt = e.timeStamp;
    props.onDown();
  };
  const pointerUp = (e: PointerEvent) => {
    if (!down) return;
    down = false;
    props.onUp(e.timeStamp - downAt);
  };
  const pointerLost = () => {
    if (!down) return;
    down = false;
    props.onLost();
  };
  // Enter: a click with no pointer behind it is a short press.
  const click = (e: MouseEvent) => {
    if (e.detail !== 0) return;
    props.onDown();
    props.onUp(0);
  };

  return (
    <button
      type="button"
      class="k-pad"
      data-state={props.state}
      aria-pressed={running()}
      aria-label={running() ? `${props.label}, running: ${RUNS[props.state]}` : props.state === "kept" ? `${props.label}, off, still held until Release` : props.label}
      disabled={props.off !== undefined}
      aria-describedby={props.off === undefined ? undefined : reason}
      onPointerDown={pointerDown}
      onPointerUp={pointerUp}
      onPointerCancel={pointerLost}
      onClick={click}
      onContextMenu={(e) => e.preventDefault()}
    >
      <span class="k-pad-field" aria-hidden="true">
        <For each={props.colours}>{(c) => <i style={{ background: c }} />}</For>
      </span>
      <Show when={props.mapped}>
        <i class="k-mapped" aria-hidden="true" />
      </Show>
      <Show when={running()}>
        <span class="k-pad-flag" aria-hidden="true">
          <Status kind="flag" label="NOW" colour="paper" />
        </span>
      </Show>
      <Show when={props.state === "kept"}>
        <span class="k-pad-flag" aria-hidden="true">
          <Status kind="pill" label="held" colour="manual" />
        </span>
      </Show>
      <span class="k-pad-name" aria-hidden="true">
        {unbroken(props.label)}
      </span>
      <Show when={props.off ?? (running() ? RUNS[props.state] : undefined)}>
        {(line) => (
          <span class="k-pad-runs" id={reason}>
            {line()}
          </span>
        )}
      </Show>
    </button>
  );
}
