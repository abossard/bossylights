import { createSignal, Show } from "solid-js";
import { keyStep, level, placeIn, type Pressable } from "./common.ts";
import { capture } from "./dragging.ts";
import { Status } from "./Status.tsx";

export interface FaderProps extends Pressable {
  label: string;
  /** 0 to 100: the value the engine reports. */
  value: number;
  onChange: (value: number) => void;
}

/**
 * A value from 0 to 100 on a 44 px track. The thumb follows the finger at once, also outside the track.
 * The number above is `value`: what came back, not what was asked for.
 */
export function Fader(props: FaderProps) {
  // The place of the finger while it is down: a drag in progress is the screen's own state.
  const [finger, setFinger] = createSignal<number>();
  const shown = () => finger() ?? level(props.value);

  const follow = (e: PointerEvent) => {
    if (!(e.currentTarget instanceof Element)) return;
    const at = placeIn(e.currentTarget.getBoundingClientRect(), e.clientX, e.clientY).up;
    setFinger(at);
    props.onChange(at);
  };
  const down = (e: PointerEvent) => {
    if (e.button > 0 || props.off !== undefined) return;
    if (e.currentTarget instanceof Element) capture(e.currentTarget, e.pointerId);
    follow(e);
  };
  const key = (e: KeyboardEvent) => {
    if (props.off !== undefined) return;
    const by = keyStep(e.key, e.shiftKey);
    if (by === undefined) return;
    e.preventDefault();
    props.onChange(by === "start" ? 0 : by === "end" ? 100 : level(props.value + by));
  };

  return (
    <div class="k-fader" classList={{ "is-off": props.off !== undefined }}>
      <span class="k-fader-value" aria-hidden="true">
        <Status kind="value" label={String(level(props.value))} {...(props.off === undefined ? {} : { colour: "mute" as const })} />
      </span>
      <div
        class="k-fader-track"
        role="slider"
        tabindex={props.off === undefined ? 0 : -1}
        aria-label={props.label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={level(props.value)}
        aria-disabled={props.off !== undefined}
        onPointerDown={down}
        onPointerMove={(e) => finger() !== undefined && follow(e)}
        onPointerUp={() => setFinger(undefined)}
        onPointerCancel={() => setFinger(undefined)}
        onKeyDown={key}
      >
        <i class="k-fader-fill" style={{ height: `${shown()}%` }} />
        <i class="k-fader-thumb" style={{ bottom: `${shown()}%` }} />
        <Show when={props.mapped}>
          <i class="k-mapped" aria-hidden="true" />
        </Show>
      </div>
      <span class="k-fader-label">{props.label}</span>
      <Show when={props.off}>{(reason) => <span class="k-reason">{reason()}</span>}</Show>
    </div>
  );
}
