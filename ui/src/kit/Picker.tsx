import { createSignal, createUniqueId, For, Show } from "solid-js";
import { neighbour, type Pressable } from "./common.ts";

export interface PickOption {
  name: string;
  /** One line under the name: what it is, or what it reads right now. Cards show it. */
  sub?: string;
  /** Why this one cannot be picked. */
  off?: string;
  /** Something behind it has trouble: a red mark. */
  marked?: boolean;
}

/**
 * | kind     | is                                          | used where                               |
 * |----------|---------------------------------------------|------------------------------------------|
 * | tabs     | all visible, the picked one a paper slab    | the workspaces, pages of a surface       |
 * | segments | one outlined group, the picked one filled   | two to five short words                  |
 * | cards    | each a box with a name and a line           | options that need a line of explanation  |
 * | list     | the picked one alone, the others open below | many options, or little room             |
 */
export type PickerKind = "tabs" | "segments" | "cards" | "list";

export interface PickerProps extends Pressable {
  kind: PickerKind;
  /** What is chosen, for screen readers. A list shows it above. */
  label: string;
  options: readonly PickOption[];
  picked: string;
  /** `small`: 30 px, for an expert screen that is not pressed during a show. */
  size?: "small" | "regular";
  onPick: (name: string) => void;
}

/** Plain names as options. */
export const named = (names: readonly string[]): PickOption[] => names.map((name) => ({ name }));

/** One of a few. Arrows move and pick. An option that is off says why, a picker that is off says why under it. */
export function Picker(props: PickerProps) {
  const open = () => (props.off === undefined ? props.options.filter((o) => o.off === undefined).map((o) => o.name) : []);
  return (
    <Show when={props.kind !== "list"} fallback={<List {...props} open={open()} />}>
      <Group {...props} open={open()} />
    </Show>
  );
}

type Inner = PickerProps & { open: readonly string[] };

/** Tabs, segments and cards: every option is on screen. */
function Group(props: Inner) {
  const tabs = () => props.kind === "tabs";
  const key = (e: KeyboardEvent) => {
    const next = neighbour(props.open, props.picked, e.key);
    if (next === undefined) return;
    e.preventDefault();
    props.onPick(next);
  };
  return (
    <div class="k-pick-wrap">
      <div class="k-pick" data-kind={props.kind} data-size={props.size ?? "regular"} classList={{ "is-off": props.off !== undefined }} role={tabs() ? "tablist" : "radiogroup"} aria-label={props.label} onKeyDown={key}>
        <For each={props.options}>
          {(option) => (
            <button
              type="button"
              class="k-pick-opt"
              role={tabs() ? "tab" : "radio"}
              aria-selected={tabs() ? option.name === props.picked : undefined}
              aria-checked={tabs() ? undefined : option.name === props.picked}
              tabindex={option.name === props.picked ? 0 : -1}
              disabled={!props.open.includes(option.name)}
              onClick={() => props.onPick(option.name)}
            >
              <span class="k-label">{option.name}</span>
              <Show when={props.kind === "cards" ? (option.off ?? option.sub) : undefined}>{(line) => <span class={option.off === undefined ? "k-sub" : "k-reason"}>{line()}</span>}</Show>
              <Show when={option.marked}>
                <i class="k-pick-mark" role="img" aria-label="has trouble" />
              </Show>
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

/**
 * A list that opens in place, drawn by the kit and not by the system. The button keeps the focus:
 * Enter, Space or an arrow opens it, arrows move, Enter picks, Escape closes and changes nothing.
 */
function List(props: Inner) {
  const id = createUniqueId();
  // Open or not, and where the arrows are: the screen's own state, gone when the list closes.
  const [at, setAt] = createSignal<string>();
  const isOpen = () => at() !== undefined;
  const close = () => setAt(undefined);
  const pick = (name: string) => {
    close();
    if (name !== props.picked) props.onPick(name);
  };
  const key = (e: KeyboardEvent) => {
    const now = at();
    if (now === undefined) {
      if (e.key !== "Enter" && e.key !== " " && e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
      e.preventDefault();
      setAt(props.open.includes(props.picked) ? props.picked : props.open[0]);
      return;
    }
    if (e.key === "Escape" || e.key === "Tab") return close();
    e.preventDefault();
    if (e.key === "Enter" || e.key === " ") return pick(now);
    if (e.key === "Home") return setAt(props.open[0]);
    if (e.key === "End") return setAt(props.open.at(-1));
    const next = neighbour(props.open, now, e.key);
    if (next !== undefined) setAt(next);
  };
  return (
    <div class="k-field k-list" data-open={isOpen() ? "" : undefined} onFocusOut={(e) => e.currentTarget.contains(e.relatedTarget as Node | null) || close()}>
      <span class="k-field-label" id={`${id}-label`}>
        {props.label}
      </span>
      <div class="k-list-box">
        <button
          type="button"
          class="k-list-button"
          aria-haspopup="listbox"
          aria-expanded={isOpen()}
          aria-labelledby={`${id}-label ${id}-value`}
          aria-activedescendant={isOpen() ? `${id}-${props.options.findIndex((o) => o.name === at())}` : undefined}
          disabled={props.off !== undefined}
          onClick={() => (isOpen() ? close() : setAt(props.open.includes(props.picked) ? props.picked : (props.open[0] ?? props.picked)))}
          onKeyDown={key}
        >
          <span class="k-label" id={`${id}-value`}>
            {props.picked}
          </span>
          <i class="k-chevron" aria-hidden="true" />
          <Show when={props.mapped}>
            <i class="k-mapped" aria-hidden="true" />
          </Show>
        </button>
        <Show when={isOpen()}>
          <div class="k-list-options" role="listbox" aria-labelledby={`${id}-label`}>
            <For each={props.options}>
              {(option, i) => (
                <div
                  class="k-list-opt"
                  classList={{ "is-at": option.name === at(), "is-off": option.off !== undefined }}
                  id={`${id}-${i()}`}
                  role="option"
                  aria-selected={option.name === props.picked}
                  aria-disabled={option.off !== undefined}
                  // The press must not take the focus from the button: the list would close before the pick.
                  onPointerDown={(e) => e.preventDefault()}
                  onPointerMove={() => option.off === undefined && setAt(option.name)}
                  onClick={() => option.off === undefined && pick(option.name)}
                >
                  <span class="k-label">{option.name}</span>
                  <Show when={option.off ?? option.sub}>{(line) => <span class={option.off === undefined ? "k-sub" : "k-reason"}>{line()}</span>}</Show>
                </div>
              )}
            </For>
          </div>
        </Show>
      </div>
      <Show when={props.off}>{(reason) => <span class="k-reason">{reason()}</span>}</Show>
    </div>
  );
}
