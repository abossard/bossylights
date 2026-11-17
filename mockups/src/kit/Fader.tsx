import { Show } from "solid-js";
import type { Layer } from "../fake/data";

export interface FaderProps {
  /** 0..100 */
  value: number;
  onChange: (value: number) => void;
  label: string;
  orientation?: "vertical" | "horizontal";
  /** Fill colour. Defaults to a neutral fill. */
  layer?: Layer;
  /** Text under (vertical) or after (horizontal) the track. Defaults to the value. */
  valueText?: string;
  /** Hide the label and value, only draw the track. */
  bare?: boolean;
  class?: string;
}

/** A fader: drag the track, or arrow keys (Shift for steps of 10). */
export function Fader(props: FaderProps) {
  let track!: HTMLDivElement;
  const vertical = () => (props.orientation ?? "vertical") === "vertical";

  const fromPointer = (e: PointerEvent) => {
    const r = track.getBoundingClientRect();
    const v = vertical() ? (r.bottom - e.clientY) / r.height : (e.clientX - r.left) / r.width;
    props.onChange(Math.round(Math.max(0, Math.min(1, v)) * 100));
  };

  const onDown = (e: PointerEvent) => {
    track.setPointerCapture(e.pointerId);
    fromPointer(e);
  };
  const onMove = (e: PointerEvent) => {
    if (track.hasPointerCapture(e.pointerId)) fromPointer(e);
  };

  const onKey = (e: KeyboardEvent) => {
    const step = e.shiftKey ? 10 : 1;
    const map: Record<string, number> = {
      ArrowUp: props.value + step,
      ArrowRight: props.value + step,
      ArrowDown: props.value - step,
      ArrowLeft: props.value - step,
      PageUp: props.value + 10,
      PageDown: props.value - 10,
      Home: 0,
      End: 100,
    };
    if (!(e.key in map)) return;
    e.preventDefault();
    props.onChange(Math.max(0, Math.min(100, map[e.key])));
  };

  return (
    <div class={`fader ${vertical() ? "fader-v" : "fader-h"} ${props.layer ? `layer-${props.layer} fader-layer` : ""} ${props.class ?? ""}`}>
      <Show when={!props.bare && vertical()}>
        <b class="fader-value display">{props.valueText ?? props.value}</b>
      </Show>
      <Show when={!props.bare && !vertical()}>
        <span class="fader-label">{props.label}</span>
      </Show>
      <div
        ref={track}
        class="fader-track"
        role="slider"
        tabIndex={0}
        aria-label={props.label}
        aria-orientation={vertical() ? "vertical" : "horizontal"}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={props.value}
        aria-valuetext={props.valueText}
        onPointerDown={onDown}
        onPointerMove={onMove}
        onKeyDown={onKey}
        style={{ "--v": `${props.value}%` }}
      >
        <i class="fader-fill" />
        <u class="fader-thumb" />
      </div>
      <Show when={!props.bare && vertical()}>
        <span class="fader-label">{props.label}</span>
      </Show>
      <Show when={!props.bare && !vertical()}>
        <b class="fader-value display">{props.valueText ?? props.value}</b>
      </Show>
    </div>
  );
}
