import { For, Show } from "solid-js";
import { neighbour, type Pressable } from "./common.ts";

export interface SegmentedProps extends Pressable {
  label: string;
  options: readonly string[];
  selected: string;
  /** 30 px high: for an expert screen that is not pressed during a show. */
  small?: boolean;
  onPick: (option: string) => void;
}

/** One outlined group, the chosen option filled. Arrows move and pick. */
export function Segmented(props: SegmentedProps) {
  const key = (e: KeyboardEvent) => {
    const next = neighbour(props.options, props.selected, e.key);
    if (next === undefined || props.off !== undefined) return;
    e.preventDefault();
    props.onPick(next);
  };
  return (
    <div class="k-seg-wrap">
      <div class="k-seg" classList={{ "is-small": props.small === true }} role="radiogroup" aria-label={props.label} onKeyDown={key}>
        <For each={props.options}>
          {(option) => (
            <button
              type="button"
              class="k-seg-opt"
              role="radio"
              aria-checked={option === props.selected}
              tabindex={option === props.selected ? 0 : -1}
              disabled={props.off !== undefined}
              onClick={() => props.onPick(option)}
            >
              {option}
            </button>
          )}
        </For>
        <Show when={props.mapped}>
          <i class="k-mapped" aria-hidden="true" />
        </Show>
      </div>
      <Show when={props.off}>{(reason) => <span class="k-reason">{reason()}</span>}</Show>
    </div>
  );
}
