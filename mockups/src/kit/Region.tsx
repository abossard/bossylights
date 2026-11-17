import { onCleanup, Show, type JSX } from "solid-js";
import { review, useScreenId } from "../review/store";

export interface RegionProps {
  /** Unique within the screen. The review layer keys notes by it. */
  id: string;
  /** Plain-words title. Shown as the heading and on the review outline. */
  title: string;
  /** Hide the visible heading. The title still shows in comment mode. */
  bare?: boolean;
  /** Content at the right end of the heading row. */
  aside?: JSX.Element;
  class?: string;
  style?: JSX.CSSProperties;
  children?: JSX.Element;
}

/** A titled part of a screen: the unit the review layer comments on. */
export function Region(props: RegionProps) {
  const screen = useScreenId();
  onCleanup(review.registerRegion(screen, props.id, props.title));
  const note = () => review.get(screen).regions[props.id]?.note.trim() ?? "";

  return (
    <section class={`region ${props.class ?? ""}`} style={props.style} data-region={props.id} aria-label={props.bare ? props.title : undefined}>
      <Show when={!props.bare}>
        <header class="region-head">
          <h2 class="display">{props.title}</h2>
          {props.aside}
        </header>
      </Show>
      {props.children}
      <Show when={review.commentMode()}>
        <button
          class="region-target"
          classList={{ "has-note": !!note() }}
          onClick={() => review.openRegion(props.id, props.title)}
          aria-label={`Note on ${props.title}`}
        >
          <span class="region-chip">
            {props.title}
            <Show when={note()}>
              <span class="region-chip-note">1 note</span>
            </Show>
          </span>
        </button>
      </Show>
    </section>
  );
}
