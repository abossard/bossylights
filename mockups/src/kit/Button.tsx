import { Show, splitProps, type JSX } from "solid-js";
import type { Layer } from "../fake/data";

export interface ButtonProps extends JSX.ButtonHTMLAttributes<HTMLButtonElement> {
  /** default: outlined. primary: marigold. danger: red (blackout only). quiet: no border. layer: the layer's colour. */
  variant?: "default" | "primary" | "danger" | "quiet" | "layer";
  layer?: Layer;
  /** Toggle buttons: shows the on state and sets aria-pressed. */
  pressed?: boolean;
  /** lg uses the display face, for the big actions pressed during a show. */
  size?: "md" | "lg";
  /** Why it is disabled, in a few words. Shown under the label while disabled. */
  reason?: string;
}

export function Button(props: ButtonProps) {
  const [own, rest] = splitProps(props, ["variant", "layer", "pressed", "size", "class", "children", "type", "reason"]);
  const why = () => (rest.disabled && own.reason ? own.reason : undefined);
  return (
    <button
      type={own.type ?? "button"}
      class={`btn btn-${own.variant ?? "default"} ${own.layer ? `layer-${own.layer}` : ""} ${own.size === "lg" ? "btn-lg display" : ""} ${own.class ?? ""}`}
      classList={{ "is-on": !!own.pressed, "has-reason": !!why() }}
      aria-pressed={own.pressed === undefined ? undefined : own.pressed}
      {...rest}
    >
      <Show when={why()} fallback={own.children}>
        <span class="btn-stack">
          <span>{own.children}</span>
          <span class="btn-reason">{why()}</span>
        </span>
      </Show>
    </button>
  );
}
