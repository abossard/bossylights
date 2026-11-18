import { createSignal, Show } from "solid-js";
import { keyStep, level, placeIn, type Pressable } from "./common.ts";
import { capture } from "./dragging.ts";

export interface XYPadProps extends Pressable {
  label: string;
  /** 0 to 100, left to right. */
  pan: number;
  /** 0 to 100, bottom to top. */
  tilt: number;
  /** The position is held by hand: the crosshair is pink. */
  held?: boolean;
  onChange: (pan: number, tilt: number) => void;
}

/** Aims: a paper crosshair and a square dot on a quiet grid. The dot follows the finger, also outside the pad. */
export function XYPad(props: XYPadProps) {
  const [down, setDown] = createSignal(false);

  const follow = (e: PointerEvent) => {
    if (!(e.currentTarget instanceof Element)) return;
    const at = placeIn(e.currentTarget.getBoundingClientRect(), e.clientX, e.clientY);
    props.onChange(at.across, at.up);
  };
  const press = (e: PointerEvent) => {
    if (e.button > 0 || props.off !== undefined) return;
    if (e.currentTarget instanceof Element) capture(e.currentTarget, e.pointerId);
    setDown(true);
    follow(e);
  };
  const key = (e: KeyboardEvent) => {
    if (props.off !== undefined) return;
    const by = keyStep(e.key, e.shiftKey);
    if (typeof by !== "number") return;
    e.preventDefault();
    const sideways = e.key === "ArrowLeft" || e.key === "ArrowRight";
    props.onChange(level(props.pan + (sideways ? by : 0)), level(props.tilt + (sideways ? 0 : by)));
  };

  return (
    <div class="k-xy-wrap">
      <div
        class="k-xy"
        classList={{ "is-held": props.held === true, "is-off": props.off !== undefined }}
        role="application"
        tabindex={props.off === undefined ? 0 : -1}
        aria-label={`${props.label}: pan ${level(props.pan)}, tilt ${level(props.tilt)}. Arrows move`}
        onPointerDown={press}
        onPointerMove={(e) => down() && follow(e)}
        onPointerUp={() => setDown(false)}
        onPointerCancel={() => setDown(false)}
        onKeyDown={key}
      >
        <i class="k-xy-h" style={{ bottom: `${level(props.tilt)}%` }} />
        <i class="k-xy-v" style={{ left: `${level(props.pan)}%` }} />
        <i class="k-xy-dot" style={{ left: `${level(props.pan)}%`, bottom: `${level(props.tilt)}%` }} />
        <Show when={props.mapped}>
          <i class="k-mapped" aria-hidden="true" />
        </Show>
      </div>
      <span class="k-xy-read">
        {props.label}: pan {level(props.pan)}, tilt {level(props.tilt)}
      </span>
      <Show when={props.off}>{(reason) => <span class="k-reason">{reason()}</span>}</Show>
    </div>
  );
}
