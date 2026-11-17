import { For } from "solid-js";
import type { Deck, Lane } from "../fake/store";
import { Decks } from "./Decks";

export interface NowStripProps {
  from: number;
  to: number;
  now: number;
  lanes: Lane[];
  /** The two decks, shown in the strip's header. */
  decks: Deck[];
  /** Clicking anywhere on the strip. The shell goes to the Console. */
  onActivate: () => void;
}

/** Three thin lanes with the NOW line, under the top bar on most screens. */
export function NowStrip(props: NowStripProps) {
  const pct = (t: number) => Math.max(0, Math.min(100, ((t - props.from) / (props.to - props.from)) * 100));
  const nowPct = () => pct(props.now);

  return (
    <button type="button" class="ns" onClick={() => props.onActivate()} aria-label="Now strip: what you hold, what the show plays, what the autopilot plays. Open the Console.">
      <span class="ns-labels" aria-hidden="true">
        <For each={props.lanes}>{(l) => <span class={`ns-label layer-${l.layer}`}>{l.title}</span>}</For>
      </span>
      <span class="ns-lanes" aria-hidden="true">
        <For each={props.lanes}>
          {(l) => (
            <span class={`ns-lane layer-${l.layer}`}>
              <For each={l.blocks}>
                {(b) => {
                  const end = () => b.end ?? props.now;
                  return (
                    <span
                      class="ns-block"
                      classList={{ "is-past": b.end !== null && b.end <= props.now }}
                      style={{ left: `${pct(b.start)}%`, width: `calc(${pct(end()) - pct(b.start)}% - 2px)` }}
                    >
                      {b.title}
                    </span>
                  );
                }}
              </For>
            </span>
          )}
        </For>
        <span class="ns-now" style={{ left: `${nowPct()}%` }} />
        <span class="now-flag ns-flag" style={{ left: `${nowPct()}%` }}>Now</span>
      </span>
      <Decks decks={props.decks} compact class="ns-decks" />
      <span class="ns-go">Open Console</span>
    </button>
  );
}
