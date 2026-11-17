import { For, type JSX } from "solid-js";

export interface TabItem {
  id: string;
  label: JSX.Element;
  /** Accessible name when the label is an icon. */
  ariaLabel?: string;
}

export interface TabsProps {
  items: TabItem[];
  value: string;
  onChange: (id: string) => void;
  /** Accessible name of the tab list. */
  label: string;
  /** workspace: the top-level tabs. page: console pages and in-screen tabs. */
  variant?: "workspace" | "page";
  class?: string;
}

/** Standard tabs: click, or arrow keys to move between them. */
export function Tabs(props: TabsProps) {
  let list!: HTMLDivElement;

  const onKey = (e: KeyboardEvent) => {
    const i = props.items.findIndex((t) => t.id === props.value);
    let next = -1;
    if (e.key === "ArrowRight") next = (i + 1) % props.items.length;
    if (e.key === "ArrowLeft") next = (i - 1 + props.items.length) % props.items.length;
    if (next < 0) return;
    e.preventDefault();
    props.onChange(props.items[next].id);
    queueMicrotask(() => (list.querySelectorAll<HTMLButtonElement>("[role=tab]")[next])?.focus());
  };

  return (
    <div ref={list} role="tablist" aria-label={props.label} class={`tabs tabs-${props.variant ?? "page"} ${props.class ?? ""}`} onKeyDown={onKey}>
      <For each={props.items}>
        {(t) => (
          <button
            role="tab"
            type="button"
            class="tab display"
            aria-selected={t.id === props.value}
            aria-label={t.ariaLabel}
            tabIndex={t.id === props.value ? 0 : -1}
            onClick={() => props.onChange(t.id)}
          >
            {t.label}
          </button>
        )}
      </For>
    </div>
  );
}
