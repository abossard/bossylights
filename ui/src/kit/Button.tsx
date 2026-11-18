import { For, Show } from "solid-js";
import type { Colour, Pressable, Signal } from "./common.ts";
import { capture } from "./dragging.ts";

/**
 * | state | means                              | look                                   |
 * |-------|------------------------------------|----------------------------------------|
 * | off   | not in effect                      | outline. A lamp is empty, its word struck |
 * | ready | will be in effect when LIVE goes on | outline. A lamp is lit mute            |
 * | on    | in effect now                      | filled in its colour. A lamp is lit    |
 */
export type ToggleState = "off" | "ready" | "on";

/**
 * How a toggle shows its state beside its fill.
 *
 * | mark    | is                         | used for                |
 * |---------|----------------------------|-------------------------|
 * | lamp    | a round lamp before the word | the four indicators   |
 * | chevron | a chevron that turns       | a part that folds       |
 * | knob    | a knob that changes side   | LIVE                    |
 */
export type ToggleMark = "lamp" | "chevron" | "knob";

interface Shared extends Pressable {
  label: string;
  /** A second, quieter line. The reason of `off` takes its place. */
  sub?: string;
  /** Whose it is. Plain: an outline, and paper when it is on. */
  colour?: Colour;
  /** A colour of its own in place of `colour`: a swatch. */
  own?: string;
  /** One sign in place of the words: "×". The label is still what a screen reader hears. */
  glyph?: string;
  /** `small`: 30 px, for an expert screen or inside another part. `large`: LIVE. */
  size?: "small" | "regular" | "large";
  /** Every line it shows in any of its states. It keeps the width of the widest, so nothing beside it moves. */
  holds?: readonly string[];
  /** What a screen reader hears, where the words on it are not enough. */
  says?: string;
  /** What plays fires on the way down. Everything else fires on the way up, so sliding off cancels. */
  fires?: "down" | "up";
  /**
   * A short press and a hold mean two things (Blackout, Learn): it reports down and up like a pad, and `onPress` is not called.
   * `heldMs`: how long it was down, 0 from the keyboard. The one who listens decides what was short.
   */
  held?: { onDown: () => void; onUp: (heldMs: number) => void };
  /** Something is dragged over it and can be dropped here: it turns paper. */
  over?: boolean;
  onPress: () => void;
}

export interface ActionProps extends Shared {
  kind?: "action";
  /** Filled: the one main action of a view, or what a press takes away (Release, Stop). */
  filled?: boolean;
}

export interface ToggleProps extends Shared {
  kind: "toggle";
  state: ToggleState;
  mark?: ToggleMark;
  /** On is shown by the mark alone: no fill. */
  quiet?: boolean;
  /** The colour of the lamp while it is on. Left out: the colour of the button, paper when plain. */
  lamp?: Signal;
}

export type ButtonProps = ActionProps | ToggleProps;

const SAYS: Record<ToggleState, string> = { off: "off", ready: "ready, starts with LIVE", on: "on" };

/**
 * The one button. Two kinds: an action fires once, a toggle stays until it is pressed again.
 * 44 px high, square. Off, it is dashed and says why under its label.
 */
export function Button(props: ButtonProps) {
  const toggle = () => (props.kind === "toggle" ? props : undefined);
  const state = () => toggle()?.state;
  const mark = () => toggle()?.mark;
  const filled = () => {
    const t = toggle();
    if (t) return t.state === "on" && t.quiet !== true;
    return props.kind !== "toggle" && props.filled === true;
  };
  const lampColour = (): Signal => {
    const t = toggle();
    if (!t || t.state !== "on") return "mute";
    return t.lamp ?? (t.colour === undefined || t.colour === "plain" ? "paper" : t.colour);
  };
  const heard = () => {
    if (props.says !== undefined) return props.says;
    const t = toggle();
    if (t && (t.mark === "lamp" || t.state === "ready")) return `${t.label}: ${SAYS[t.state]}`;
    return props.glyph === undefined ? undefined : props.label;
  };

  let downAt = 0;
  let isDown = false;
  const down = (e: PointerEvent) => {
    if (e.button !== 0) return;
    if (props.held === undefined) {
      if (props.fires === "down") props.onPress();
      return;
    }
    if (e.currentTarget instanceof Element) capture(e.currentTarget, e.pointerId);
    isDown = true;
    downAt = e.timeStamp;
    props.held.onDown();
  };
  const up = (e: PointerEvent) => {
    if (!isDown) return;
    isDown = false;
    props.held?.onUp(e.timeStamp - downAt);
  };
  // A click with no pointer behind it (detail 0) is Enter or Space: a short press.
  const click = (e: MouseEvent) => {
    if (props.held !== undefined) {
      if (e.detail !== 0) return;
      props.held.onDown();
      props.held.onUp(0);
      return;
    }
    if (props.fires !== "down" || e.detail === 0) props.onPress();
  };

  return (
    <button
      type="button"
      class="k-btn"
      classList={{ "is-filled": filled(), "is-over": props.over === true, "is-glyph": props.glyph !== undefined }}
      data-kind={props.kind ?? "action"}
      data-colour={props.colour ?? "plain"}
      data-size={props.size ?? "regular"}
      data-mark={mark()}
      data-state={state()}
      style={props.own === undefined ? {} : { "--own": props.own }}
      role={mark() === "knob" ? "switch" : undefined}
      aria-checked={mark() === "knob" ? state() === "on" : undefined}
      aria-expanded={mark() === "chevron" ? state() === "on" : undefined}
      aria-pressed={toggle() && mark() !== "knob" && mark() !== "chevron" ? state() !== "off" : undefined}
      aria-label={heard()}
      disabled={props.off !== undefined}
      onPointerDown={down}
      onPointerUp={up}
      onPointerCancel={up}
      onClick={click}
    >
      <Show when={props.mapped}>
        <i class="k-mapped" aria-hidden="true" />
      </Show>
      <Show when={mark() === "lamp"}>
        <i class="k-lamp" data-colour={lampColour()} data-fill={state() === "off" ? "outline" : "solid"} aria-hidden="true" />
      </Show>
      <Show when={mark() === "chevron"}>
        <i class="k-chevron" aria-hidden="true" />
      </Show>
      <span class="k-btn-text">
        <span class="k-label">{props.glyph ?? props.label}</span>
        <Show when={props.off ?? props.sub}>{(line) => <span class="k-reason">{line()}</span>}</Show>
        <For each={props.holds ?? []}>{(line) => <span class="k-ghost" data-line={line} aria-hidden="true" />}</For>
      </span>
      <Show when={mark() === "knob"}>
        <i class="k-knob" aria-hidden="true" />
      </Show>
    </button>
  );
}
