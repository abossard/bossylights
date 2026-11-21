import { createSignal, createUniqueId, Show } from "solid-js";
import { keyStep, level, placeIn, scaled, type Pressable } from "./common.ts";
import { capture } from "./dragging.ts";
import { Status } from "./Status.tsx";

export interface FaderProps extends Pressable {
  label: string;
  /** The top of the range: 100 for a level in percent, 255 for a raw DMX channel. The bottom is 0. */
  most: number;
  /** 0 to `most`: the value the engine reports. */
  value: number;
  onChange: (value: number) => void;
}

/**
 * A value from 0 to `most` on a 44 px track. The thumb follows the finger at once, also outside the track.
 * The number above is `value`: what came back, not what was asked for.
 */
export function Fader(props: FaderProps) {
  const reason = createUniqueId();
  // The place of the finger while it is down: a drag in progress is the screen's own state.
  const [finger, setFinger] = createSignal<number>();
  const value = () => scaled(props.value, props.most);
  // The fill in percent of the track.
  const shown = () => finger() ?? level((value() * 100) / props.most);

  const follow = (e: PointerEvent) => {
    if (!(e.currentTarget instanceof Element)) return;
    const at = placeIn(e.currentTarget.getBoundingClientRect(), e.clientX, e.clientY).up;
    setFinger(at);
    props.onChange(scaled((at * props.most) / 100, props.most));
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
    props.onChange(by === "start" ? 0 : by === "end" ? props.most : scaled(value() + by, props.most));
  };

  return (
    <div class="k-fader" classList={{ "is-off": props.off !== undefined }}>
      <span class="k-fader-value" aria-hidden="true">
        <Status kind="value" label={String(value())} {...(props.off === undefined ? {} : { colour: "mute" as const })} />
      </span>
      <div
        class="k-fader-track"
        role="slider"
        // Off, it keeps the focus: the keyboard hears why.
        tabindex={0}
        aria-label={props.label}
        aria-describedby={props.off === undefined ? undefined : reason}
        aria-valuemin={0}
        aria-valuemax={props.most}
        aria-valuenow={value()}
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
      <Show when={props.off}>
        {(why) => (
          <span class="k-reason" id={reason}>
            {why()}
          </span>
        )}
      </Show>
    </div>
  );
}
