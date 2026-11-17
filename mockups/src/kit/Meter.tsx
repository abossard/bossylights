import type { Layer } from "../fake/data";

export interface MeterProps {
  /** 0..1 */
  value: number;
  /** Accessible name. */
  label: string;
  layer?: Layer;
  /** "danger" for clipping or errors. */
  tone?: "paper" | "danger";
  orientation?: "horizontal" | "vertical";
  class?: string;
}

/** A read-only level bar. */
export function Meter(props: MeterProps) {
  return (
    <span
      class={`meter meter-${props.orientation ?? "horizontal"} meter-${props.tone ?? "paper"} ${props.layer ? `layer-${props.layer} meter-layer` : ""} ${props.class ?? ""}`}
      role="meter"
      aria-label={props.label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(props.value * 100)}
      style={{ "--v": `${Math.max(0, Math.min(1, props.value)) * 100}%` }}
    >
      <i />
    </span>
  );
}
