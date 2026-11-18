import { For, Show } from "solid-js";
import { neighbour } from "./common.ts";

export interface TabsProps {
  /** What the tabs choose between, for screen readers. */
  label: string;
  options: readonly string[];
  selected: string;
  /** Tabs that carry a red mark: something behind them has trouble. */
  marked?: readonly string[];
  onPick: (option: string) => void;
}

/** One of a few, all visible. The selected tab is a paper slab. Arrows move and pick. */
export function Tabs(props: TabsProps) {
  const key = (e: KeyboardEvent) => {
    const next = neighbour(props.options, props.selected, e.key);
    if (next === undefined) return;
    e.preventDefault();
    props.onPick(next);
  };
  return (
    <div class="k-tabs" role="tablist" aria-label={props.label} onKeyDown={key}>
      <For each={props.options}>
        {(option) => (
          <button type="button" class="k-tab" role="tab" aria-selected={option === props.selected} tabindex={option === props.selected ? 0 : -1} onClick={() => props.onPick(option)}>
            {option}
            <Show when={props.marked?.includes(option)}>
              <i class="k-tab-mark" role="img" aria-label="has trouble" />
            </Show>
          </button>
        )}
      </For>
    </div>
  );
}
