import { createEffect, createSignal, createUniqueId, For, Show, type JSX } from "solid-js";
import { neighbour, type Pressable } from "./common.ts";

export interface PickOption {
  name: string;
  /** One line under the name: what it is, or what it reads right now. Cards show it. */
  sub?: string;
  /** Why this one cannot be picked. */
  off?: string;
  /** Something behind it has trouble: a red mark. */
  marked?: boolean;
  /** What it looks like, drawn above its name: a gobo, a slot of a colour wheel (`Disc`), an effect (`Preview`). Pictures show it. */
  picture?: () => JSX.Element;
}

/** A slot of a colour wheel as a picture: one colour, or two side by side for a split slot. */
export function Disc(props: { colours: readonly string[] }) {
  const fill = () => (props.colours.length > 1 ? `linear-gradient(90deg, ${props.colours[0] ?? "#000"} 50%, ${props.colours[1] ?? "#000"} 50%)` : (props.colours[0] ?? "#000"));
  return <i class="k-disc" style={{ background: fill() }} />;
}

/**
 * | kind     | is                                          | used where                               |
 * |----------|---------------------------------------------|------------------------------------------|
 * | tabs     | all visible, the picked one a paper slab    | the workspaces, pages of a surface       |
 * | segments | one outlined group, the picked one filled   | two to five short words                  |
 * | cards    | each a box with a name and a line           | options that need a line of explanation  |
 * | pictures | each a drawing with its name under it       | things known by their look: gobos, the slots of a colour wheel, effects |
 * | list     | the picked one alone, the others open below | many options, or little room             |
 *
 * Tabs that do not fit in a line scroll sideways, the picked one stays in view. `after` puts one action behind them.
 */
export type PickerKind = "tabs" | "segments" | "cards" | "pictures" | "list";

export interface PickerProps extends Pressable {
  kind: PickerKind;
  /** What is chosen, for screen readers. A list shows it above. */
  label: string;
  options: readonly PickOption[];
  picked: string;
  /** `small`: 30 px, for an expert screen that is not pressed during a show. */
  size?: "small" | "regular";
  /** A choice that plays picks on the way down. Every other one on the way up. A list always picks on the way up. */
  fires?: "down" | "up";
  /** Tabs only: one action after the last tab, as "Add page". It is a button, never a tab. */
  after?: { label: string; onPress: () => void };
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
  const key = (e: KeyboardEvent & { currentTarget: HTMLElement }) => {
    const next = neighbour(props.open, props.picked, e.key);
    if (next === undefined) return;
    e.preventDefault();
    props.onPick(next);
    // The picked option takes the focus: the next arrow goes on from there.
    const at = props.options.findIndex((o) => o.name === next);
    e.currentTarget.querySelectorAll<HTMLElement>(":scope > .k-pick-opt")[at]?.focus();
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
              ref={(el) => createEffect(() => tabs() && option.name === props.picked && el.scrollIntoView({ block: "nearest", inline: "nearest" }))}
              disabled={!props.open.includes(option.name)}
              onPointerDown={(e) => props.fires === "down" && e.button === 0 && props.onPick(option.name)}
              // A click with no pointer behind it (detail 0) is Enter or Space.
              onClick={(e) => (props.fires !== "down" || e.detail === 0) && props.onPick(option.name)}
            >
              <Show when={props.kind === "pictures" ? option.picture : undefined}>
                {(draw) => (
                  <span class="k-pick-picture" aria-hidden="true">
                    {draw()()}
                  </span>
                )}
              </Show>
              <span class="k-label">{option.name}</span>
              <Show when={props.kind === "cards" ? (option.off ?? option.sub) : props.kind === "pictures" ? option.off : undefined}>{(line) => <span class={option.off === undefined ? "k-sub" : "k-reason"}>{line()}</span>}</Show>
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
      <Show when={tabs() ? props.after : undefined}>
        {(after) => (
          <button type="button" class="k-pick-after" disabled={props.off !== undefined} onClick={() => after().onPress()}>
            {after().label}
          </button>
        )}
      </Show>
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
