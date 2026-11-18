import { Show } from "solid-js";

export interface FoldProps {
  label: string;
  /** How many things are inside. */
  count?: number;
  open: boolean;
  /** Something is dragged over it: it turns paper. */
  over?: boolean;
  onToggle: (next: boolean) => void;
}

/** The header of a part that folds: a chevron, its name, how much is inside. 44 px high. */
export function Fold(props: FoldProps) {
  return (
    <div class="k-fold-wrap">
      <button type="button" class="k-fold" classList={{ "is-over": props.over === true }} aria-expanded={props.open} onClick={() => props.onToggle(!props.open)}>
        <i class="k-chevron" classList={{ "is-open": props.open }} aria-hidden="true" />
        <span class="k-label">{props.label}</span>
        <Show when={props.count !== undefined}>
          <span class="k-sub">{props.count}</span>
        </Show>
      </button>
      <Show when={props.open && props.count === 0}>
        <span class="k-fold-empty">Empty. Drag a look here.</span>
      </Show>
    </div>
  );
}
