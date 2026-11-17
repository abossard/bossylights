import { For } from "solid-js";
import type { Layer } from "../fake/data";

export interface SegmentedProps<T extends string> {
  options: { value: T; label: string }[];
  value: T;
  onChange: (value: T) => void;
  /** Accessible name of the group. */
  label: string;
  /** Colour of the chosen segment. Defaults to paper. */
  layer?: Layer;
  size?: "md" | "lg";
  class?: string;
}

/** One choice out of a few, all visible. */
export function Segmented<T extends string>(props: SegmentedProps<T>) {
  const onKey = (e: KeyboardEvent) => {
    const i = props.options.findIndex((o) => o.value === props.value);
    let next = -1;
    if (e.key === "ArrowRight" || e.key === "ArrowDown") next = (i + 1) % props.options.length;
    if (e.key === "ArrowLeft" || e.key === "ArrowUp") next = (i - 1 + props.options.length) % props.options.length;
    if (next < 0) return;
    e.preventDefault();
    props.onChange(props.options[next].value);
    const group = e.currentTarget as HTMLElement;
    queueMicrotask(() => group.querySelectorAll<HTMLButtonElement>("[role=radio]")[next]?.focus());
  };

  return (
    <div
      role="radiogroup"
      aria-label={props.label}
      class={`seg ${props.layer ? `layer-${props.layer} seg-layer` : ""} ${props.size === "lg" ? "seg-lg" : ""} ${props.class ?? ""}`}
      onKeyDown={onKey}
    >
      <For each={props.options}>
        {(o) => (
          <button
            type="button"
            role="radio"
            class="seg-opt display"
            aria-checked={o.value === props.value}
            tabIndex={o.value === props.value ? 0 : -1}
            onClick={() => props.onChange(o.value)}
          >
            {o.label}
          </button>
        )}
      </For>
    </div>
  );
}
