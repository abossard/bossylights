import { Show } from "solid-js";

/** The lamp of the item: `mute` the engine runs, `paper` LIVE is on, `danger` trouble, `empty` not connected. */
export type MenuLamp = "mute" | "paper" | "danger" | "empty";

export interface MenuItemProps {
  label: string;
  /** `item`: what sits in the menu bar. `entry`: one line of its menu. */
  is: "item" | "entry";
  lamp?: MenuLamp;
  /** A quieter word after the label: the state in words, the frame counter. */
  sub?: string;
  /** The menu of the item is open. */
  open?: boolean;
  /** Why an entry cannot be picked. A menu of the system says it after the label. */
  off?: string;
  onPress: () => void;
}

/**
 * The menu bar item and the entries of its menu. The system draws the real ones: this is their likeness,
 * for the catalog. An item opens on the way down, an entry fires on the way up, as every menu does.
 */
export function MenuItem(props: MenuItemProps) {
  const down = (e: PointerEvent) => {
    if (props.is === "item" && e.button === 0) props.onPress();
  };
  const click = (e: MouseEvent) => {
    if (props.is === "entry" || e.detail === 0) props.onPress();
  };
  return (
    <button
      type="button"
      class="k-menu"
      data-is={props.is}
      classList={{ "is-on": props.open === true }}
      role={props.is === "entry" ? "menuitem" : undefined}
      aria-haspopup={props.is === "item" ? "menu" : undefined}
      aria-expanded={props.is === "item" ? props.open === true : undefined}
      disabled={props.off !== undefined}
      onPointerDown={down}
      onClick={click}
    >
      <Show when={props.lamp}>{(lamp) => <i class="k-lamp" data-lamp={lamp()} aria-hidden="true" />}</Show>
      <span class="k-label">{props.label}</span>
      <Show when={props.off ?? props.sub}>{(line) => <span class="k-sub">{line()}</span>}</Show>
    </button>
  );
}
