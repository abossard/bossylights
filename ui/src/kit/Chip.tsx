import { Show } from "solid-js";

export interface ChipProps {
  label: string;
  swatch?: string;
  /** The cross takes it out of this place. It never deletes the thing itself. */
  onRemove: () => void;
}

/** A thing in a place, with a cross to take it out of that place. */
export function Chip(props: ChipProps) {
  return (
    <span class="k-chip">
      <Show when={props.swatch}>{(colour) => <i class="k-row-swatch" style={{ background: colour() }} aria-hidden="true" />}</Show>
      <span class="k-label">{props.label}</span>
      <button type="button" class="k-chip-x" aria-label={`Take ${props.label} out`} onClick={() => props.onRemove()}>
        <span aria-hidden="true">×</span>
      </button>
    </span>
  );
}
