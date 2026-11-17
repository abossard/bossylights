import { For, Show, type JSX } from "solid-js";
import type { Layer } from "../fake/data";

export interface ListItem {
  id: string;
  title: JSX.Element;
  sub?: JSX.Element;
  /** Right-aligned value or state. */
  meta?: JSX.Element;
  /** Colour chip at the start. */
  swatch?: string;
  /** Marks the row as belonging to a layer (its colour on the edge chip). */
  layer?: Layer;
  /** Extra controls at the end of the row. */
  actions?: JSX.Element;
}

export interface ListProps {
  items: ListItem[];
  /** Makes rows selectable. */
  onSelect?: (id: string) => void;
  selected?: string | null;
  /** Shown when there are no items: say what will appear and how. */
  empty?: JSX.Element;
  label?: string;
  class?: string;
}

/** A plain list with 44 px rows. */
export function List(props: ListProps) {
  return (
    <Show when={props.items.length} fallback={<p class="list-empty">{props.empty}</p>}>
      <ul class={`list ${props.class ?? ""}`} aria-label={props.label}>
        <For each={props.items}>
          {(item) => {
            const body = (
              <>
                <Show when={item.swatch || item.layer}>
                  <i
                    class={`list-swatch ${item.layer ? `layer-${item.layer}` : ""}`}
                    style={{ background: item.swatch ?? "var(--layer)" }}
                  />
                </Show>
                <span class="list-text">
                  <span class="list-title">{item.title}</span>
                  <Show when={item.sub}>
                    <span class="list-sub">{item.sub}</span>
                  </Show>
                </span>
                <Show when={item.meta}>
                  <span class="list-meta">{item.meta}</span>
                </Show>
              </>
            );
            return (
              <li class="list-row" classList={{ "is-selected": props.selected === item.id }}>
                <Show when={props.onSelect} fallback={<div class="list-body">{body}</div>}>
                  <button
                    type="button"
                    class="list-body"
                    aria-current={props.selected === item.id ? "true" : undefined}
                    onClick={() => props.onSelect?.(item.id)}
                  >
                    {body}
                  </button>
                </Show>
                {item.actions}
              </li>
            );
          }}
        </For>
      </ul>
    </Show>
  );
}
