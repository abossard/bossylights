import { For, Show } from "solid-js";
import { neighbour } from "./common.ts";

export interface ChoiceOption {
  name: string;
  /** One line under the name: what it is, or what it reads right now. */
  sub: string;
  /** Why this one cannot be picked. */
  off?: string;
}

export interface ChoiceProps {
  label: string;
  options: readonly ChoiceOption[];
  selected: string;
  onPick: (name: string) => void;
}

/** One of a few, each a card with a name and a line. The picked card is paper. An option that is off says why. */
export function Choice(props: ChoiceProps) {
  const open = () => props.options.filter((o) => o.off === undefined).map((o) => o.name);
  const key = (e: KeyboardEvent) => {
    const next = neighbour(open(), props.selected, e.key);
    if (next === undefined) return;
    e.preventDefault();
    props.onPick(next);
  };
  return (
    <div class="k-choice" role="radiogroup" aria-label={props.label} onKeyDown={key}>
      <For each={props.options}>
        {(option) => (
          <button
            type="button"
            class="k-card"
            role="radio"
            aria-checked={option.name === props.selected}
            tabindex={option.name === props.selected ? 0 : -1}
            disabled={option.off !== undefined}
            onClick={() => props.onPick(option.name)}
          >
            <span class="k-label">{option.name}</span>
            <Show when={option.off} fallback={<span class="k-sub">{option.sub}</span>}>
              {(reason) => <span class="k-reason">{reason()}</span>}
            </Show>
          </button>
        )}
      </For>
    </div>
  );
}
