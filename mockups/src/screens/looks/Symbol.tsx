import { Show } from "solid-js";
import type { Item, Shape } from "../../fake/store";

/** One symbol per kind: a square for a step, bars for a sequence, a wave for a pattern. */
export function KindSymbol(props: { kind: Item["kind"]; size?: number }) {
  const s = () => props.size ?? 18;
  return (
    <svg class={`lk-sym lk-sym-${props.kind}`} width={s()} height={s()} viewBox="0 0 18 18" aria-hidden="true">
      <Show when={props.kind === "step"}>
        <rect x="3" y="3" width="12" height="12" fill="currentColor" />
      </Show>
      <Show when={props.kind === "sequence"}>
        <path d="M2 4h5v3H2zM7 8h5v3H7zM12 12h4v3h-4z" fill="currentColor" />
      </Show>
      <Show when={props.kind === "pattern"}>
        <path d="M1 12c3 0 3-7 6-7s3 7 6 7 3-4 4-4" fill="none" stroke="currentColor" stroke-width="2.5" />
      </Show>
    </svg>
  );
}

export const KIND_NAME: Record<Item["kind"], string> = { step: "Step", sequence: "Sequence", pattern: "Pattern" };

/** A still picture of a pattern shape. Not a preview: nothing moves. */
export function ShapePicture(props: { shape: Shape }) {
  return (
    <svg class="lk-shape-pic" viewBox="0 0 64 32" aria-hidden="true">
      <Show when={props.shape === "plasma"}>
        <circle cx="16" cy="14" r="10" />
        <circle cx="38" cy="20" r="9" />
        <circle cx="54" cy="10" r="7" />
      </Show>
      <Show when={props.shape === "stripes"}>
        <path d="M4 4h8v24H4zM20 4h8v24h-8zM36 4h8v24h-8zM52 4h8v24h-8z" />
      </Show>
      <Show when={props.shape === "wave"}>
        <path class="lk-shape-line" d="M2 16c6-12 10-12 15 0s9 12 15 0 10-12 15 0 9 12 15 0" />
      </Show>
      <Show when={props.shape === "sparkle"}>
        <path d="M8 6h4v4H8zM26 18h4v4h-4zM44 8h4v4h-4zM52 22h4v4h-4zM16 22h4v4h-4zM36 2h4v4h-4z" />
      </Show>
    </svg>
  );
}

/** A gobo, drawn as its cut-out. */
export function GoboPicture(props: { gobo: string }) {
  return (
    <svg class="lk-gobo-pic" viewBox="0 0 40 40" aria-hidden="true">
      <circle cx="20" cy="20" r="18" class="lk-gobo-disc" />
      <Show when={props.gobo === "leaf"}>
        <path d="M10 28C10 14 20 8 30 10c2 10-4 20-20 18zM12 26l14-12" class="lk-gobo-cut" />
      </Show>
      <Show when={props.gobo === "dots"}>
        <path d="M12 12h5v5h-5zM23 12h5v5h-5zM12 23h5v5h-5zM23 23h5v5h-5z" class="lk-gobo-cut" />
      </Show>
      <Show when={props.gobo === "ring"}>
        <circle cx="20" cy="20" r="10" class="lk-gobo-ring" />
      </Show>
      <Show when={props.gobo === "star"}>
        <path d="M20 8l3 9h9l-7 5 3 9-8-6-8 6 3-9-7-5h9z" class="lk-gobo-cut" />
      </Show>
      <Show when={props.gobo === "bars"}>
        <path d="M10 10h4v20h-4zM18 10h4v20h-4zM26 10h4v20h-4z" class="lk-gobo-cut" />
      </Show>
      <Show when={props.gobo === "flower"}>
        <path d="M20 9a5 5 0 0 1 0 10a5 5 0 0 1 0-10zM20 21a5 5 0 0 1 0 10a5 5 0 0 1 0-10zM9 20a5 5 0 0 1 10 0a5 5 0 0 1-10 0zM21 20a5 5 0 0 1 10 0a5 5 0 0 1-10 0z" class="lk-gobo-cut" />
      </Show>
    </svg>
  );
}
