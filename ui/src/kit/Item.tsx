import { For, Show } from "solid-js";
import { Button } from "./Button.tsx";
import { offWith, type Colour, type Pressable, type Standing } from "./common.ts";
import { dragHandlers } from "./dragging.ts";
import { Status, type StatusProps } from "./Status.tsx";

/**
 * | kind  | is                                            | used for                                   |
 * |-------|-----------------------------------------------|--------------------------------------------|
 * | row   | one line of a list, 44 px                     | a song, a look, a light, an output         |
 * | tile  | a box in a picture, a small word for what it is | what a look is made of, a module         |
 * | cell  | one square of a grid with its reading         | a channel of a universe                    |
 * | entry | one line of a menu                            | the menu bar item and its menu             |
 * | chip  | a thing in a place, with a cross to take it out | a look on a section, a group on a look   |
 * | strip | a full line of statuses                       | the Now strip                              |
 */
export type ItemKind = "row" | "tile" | "cell" | "entry" | "chip" | "strip";

export interface ItemProps extends Pressable {
  kind: ItemKind;
  title: string;
  /** A second, quieter line: what it is, where it is. The reason of `off` takes its place. */
  sub?: string;
  /** Its own colour as a small field before the title. */
  swatch?: string;
  /** A status before the title: a lamp, the NOW flag. */
  lead?: StatusProps;
  /** Statuses after the title: its state in a word, its reading, what runs in a lane. */
  marks?: readonly StatusProps[];
  /** Whose it is: a fill. A channel held by hand is pink. */
  colour?: Colour;
  standing?: Standing;
  /** What a screen reader hears, where the words on it are not enough. */
  says?: string;
  /** An entry: one of an open menu. Elsewhere it is a button. */
  inMenu?: boolean;
  /** An entry that opens a menu: whether it is open. */
  expanded?: boolean;
  /** A press picks or opens it. A chip has none: it is only taken out. */
  onPress?: () => void;
  /** The cross of a chip takes it out of this place. It never deletes the thing itself. */
  onRemove?: () => void;
  /** It can be dragged somewhere: called when it is dropped. */
  onDrop?: () => void;
  onLift?: () => void;
  onCancel?: () => void;
}

/** A thing, shown where it lives. A press picks or opens it, a drag takes it somewhere else. */
export function Item(props: ItemProps) {
  const drag = dragHandlers(() => ({
    // A pointer press comes from the drag machine: after a drop there is no press.
    ...(props.onPress ? { onPress: props.onPress } : {}),
    ...(props.onDrop ? { onDrop: props.onDrop } : {}),
    ...(props.onLift ? { onMove: props.onLift } : {}),
    ...(props.onCancel ? { onCancel: props.onCancel } : {}),
  }));
  const inside = () => (
    <>
      <Show when={props.mapped}>
        <i class="k-mapped" aria-hidden="true" />
      </Show>
      <Show when={props.swatch}>{(colour) => <i class="k-item-swatch" style={{ background: colour() }} aria-hidden="true" />}</Show>
      <Show when={props.lead}>{(lead) => <Status {...lead()} />}</Show>
      <span class="k-item-text">
        <span class="k-label">{props.title}</span>
        <Show when={props.off ?? props.sub}>{(line) => <span class={props.off === undefined ? "k-sub" : "k-reason"}>{line()}</span>}</Show>
      </span>
      <For each={props.marks ?? []}>{(mark) => <Status {...mark} />}</For>
    </>
  );
  const shared = () => ({
    "data-kind": props.kind,
    "data-colour": props.colour ?? "plain",
    "data-standing": props.standing ?? "plain",
  });

  return (
    <Show
      when={props.kind !== "chip"}
      fallback={
        <span class="k-item" {...shared()}>
          {inside()}
          <Button label={`Take ${props.title} out`} glyph="×" {...offWith(props.off)} onPress={() => props.onRemove?.()} />
        </span>
      }
    >
      <button
        type="button"
        class="k-item"
        {...shared()}
        role={props.kind === "entry" && props.inMenu ? "menuitem" : undefined}
        aria-haspopup={props.expanded === undefined ? undefined : "menu"}
        aria-expanded={props.expanded}
        aria-current={props.standing === "picked"}
        aria-label={props.says}
        disabled={props.off !== undefined}
        onPointerDown={(e) => props.onDrop && drag.onPointerDown(e)}
        onPointerMove={drag.onPointerMove}
        onPointerUp={drag.onPointerUp}
        onPointerCancel={drag.onPointerCancel}
        onKeyDown={drag.onKeyDown}
        onClick={(e) => (e.detail === 0 || !props.onDrop) && props.onPress?.()}
      >
        {inside()}
      </button>
    </Show>
  );
}
