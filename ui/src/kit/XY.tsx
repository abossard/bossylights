import { createSignal, Show } from "solid-js";
import { keyStep, level, placeIn, wheelAt, wheelColour, wheelSpot, type Pressable } from "./common.ts";
import { capture } from "./dragging.ts";

/**
 * | kind     | is                                              | the two values        |
 * |----------|-------------------------------------------------|-----------------------|
 * | position | a plane: left to right, bottom to top           | pan, tilt             |
 * | colour   | a wheel: the hue around, its strength outward   | hue, strength         |
 */
export type XYKind = "position" | "colour";

export interface XYProps extends Pressable {
  kind?: XYKind;
  label: string;
  /** 0 to 100. Position: left to right. Colour: around the wheel, red at the top. */
  across: number;
  /** 0 to 100. Position: bottom to top. Colour: from the middle outward. */
  up: number;
  /** The position is held by hand: the crosshair is pink. */
  held?: boolean;
  onChange: (across: number, up: number) => void;
}

const NAMES: Record<XYKind, readonly [string, string]> = { position: ["pan", "tilt"], colour: ["hue", "strength"] };

/**
 * Two values at once, under one finger. A position: a paper crosshair and a square dot on a quiet grid.
 * A colour: a wheel with a ring where the colour is. The mark follows the finger, also outside.
 */
export function XY(props: XYProps) {
  const [down, setDown] = createSignal(false);
  const kind = () => props.kind ?? "position";
  const wheel = () => kind() === "colour";
  const reads = () => `${props.label}: ${NAMES[kind()][0]} ${level(props.across)}, ${NAMES[kind()][1]} ${level(props.up)}`;

  const follow = (e: PointerEvent) => {
    if (!(e.currentTarget instanceof Element)) return;
    const box = e.currentTarget.getBoundingClientRect();
    if (wheel()) {
      const at = wheelAt(box, e.clientX, e.clientY);
      props.onChange(at.around, at.out);
    } else {
      const at = placeIn(box, e.clientX, e.clientY);
      props.onChange(at.across, at.up);
    }
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
    // Around a wheel there is no end: the hue goes on past red.
    const across = wheel() ? (props.across + by + 100) % 100 : level(props.across + by);
    props.onChange(sideways ? across : props.across, sideways ? props.up : level(props.up + by));
  };
  const spot = () => wheelSpot(props.across, props.up);

  return (
    <div class="k-xy-wrap">
      <div
        class="k-xy"
        data-kind={kind()}
        classList={{ "is-held": props.held === true, "is-off": props.off !== undefined }}
        role="application"
        tabindex={props.off === undefined ? 0 : -1}
        aria-label={`${reads()}. Arrows move`}
        onPointerDown={press}
        onPointerMove={(e) => down() && follow(e)}
        onPointerUp={() => setDown(false)}
        onPointerCancel={() => setDown(false)}
        onKeyDown={key}
      >
        <Show
          when={wheel()}
          fallback={
            <>
              <i class="k-xy-h" style={{ bottom: `${level(props.up)}%` }} />
              <i class="k-xy-v" style={{ left: `${level(props.across)}%` }} />
              <i class="k-xy-dot" style={{ left: `${level(props.across)}%`, bottom: `${level(props.up)}%` }} />
            </>
          }
        >
          <i class="k-xy-ring" style={{ left: `${spot().left}%`, top: `${spot().top}%`, background: wheelColour(props.across, props.up) }} />
        </Show>
        <Show when={props.mapped}>
          <i class="k-mapped" aria-hidden="true" />
        </Show>
      </div>
      <span class="k-xy-read">
        <Show when={wheel()}>
          <i class="k-xy-swatch" style={{ background: wheelColour(props.across, props.up) }} aria-hidden="true" />
        </Show>
        {reads()}
      </span>
      <Show when={props.off}>{(reason) => <span class="k-reason">{reason()}</span>}</Show>
    </div>
  );
}
