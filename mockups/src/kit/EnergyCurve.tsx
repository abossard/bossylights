import { For, Show } from "solid-js";
import type { Layer } from "../fake/data";

export interface EnergySpan {
  start: number;
  end: number;
  /** 0..1 */
  energy: number;
  title: string;
}

export interface EnergyCurveProps {
  spans: EnergySpan[];
  from: number;
  to: number;
  /** The NOW line. Outside from..to it is not drawn. */
  now: number;
  layer?: Layer;
  label: string;
  class?: string;
}

/** How hard the music hits over time, per section, with the NOW line. */
export function EnergyCurve(props: EnergyCurveProps) {
  const x = (t: number) => ((t - props.from) / (props.to - props.from)) * 1000;
  const y = (e: number) => 100 - e * 88;

  const path = () => {
    const pts: string[] = [];
    props.spans.forEach((s, i) => {
      const a = x(s.start);
      const b = x(s.end);
      const ramp = Math.min(40, (b - a) / 4);
      pts.push(`${i === 0 ? "M" : "L"}${a + (i === 0 ? 0 : ramp)},${y(s.energy)}`, `L${b - ramp},${y(s.energy)}`);
    });
    const last = props.spans[props.spans.length - 1];
    if (last) pts.push(`L${x(last.end)},${y(last.energy)}`);
    return pts.join(" ");
  };
  const area = () => {
    const first = props.spans[0];
    const last = props.spans[props.spans.length - 1];
    return first && last ? `${path()} L${x(last.end)},100 L${x(first.start)},100 Z` : "";
  };
  const nowPct = () => Math.max(0, Math.min(100, x(props.now) / 10));

  return (
    <div class={`energy ${props.layer ? `layer-${props.layer}` : ""} ${props.class ?? ""}`} role="img" aria-label={props.label}>
      <svg viewBox="0 0 1000 100" preserveAspectRatio="none">
        <For each={props.spans.slice(1)}>
          {(s) => <line class="energy-div" x1={x(s.start)} x2={x(s.start)} y1="0" y2="100" vector-effect="non-scaling-stroke" />}
        </For>
        <path class="energy-area" d={area()} />
        <path class="energy-line" d={path()} vector-effect="non-scaling-stroke" />
      </svg>
      <Show when={props.now >= props.from && props.now <= props.to}>
        <span class="energy-now" style={{ left: `${nowPct()}%` }} />
      </Show>
    </div>
  );
}
