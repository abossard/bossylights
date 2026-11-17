import { Index, Show } from "solid-js";
import { mmss, type Deck } from "../fake/store";

export interface DecksProps {
  decks: Deck[];
  /** One line per deck, for the Now strip. */
  compact?: boolean;
  class?: string;
}

function PlayIcon(props: { playing: boolean }) {
  return (
    <svg class="deck-icon" width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
      <Show when={props.playing} fallback={<path d="M3 2h3v10H3zM8 2h3v10H8z" fill="currentColor" />}>
        <path d="M3 1.5l9 5.5-9 5.5z" fill="currentColor" />
      </Show>
    </svg>
  );
}

/**
 * The two VirtualDJ decks: letter, playing or paused, song title and artist,
 * tempo, position, and which one is on air. The on-air title is the largest.
 */
export function Decks(props: DecksProps) {
  return (
    <div class={`decks ${props.compact ? "decks-compact" : ""} ${props.class ?? ""}`} aria-label="Decks">
      <Index each={props.decks}>
        {(d) => (
          <div
            class="deck"
            classList={{ "is-onair": d().onAir }}
            aria-label={`Deck ${d().letter}, ${d().playing ? "playing" : "paused"}${d().onAir ? ", on air" : ""}: ${d().song.title} by ${d().song.artist}`}
          >
            <span class="deck-letter display">{d().letter}</span>
            <PlayIcon playing={d().playing} />
            <span class="deck-song">
              <b class="deck-title display">{d().song.title}</b>
              <span class="deck-meta">
                {d().song.artist}
                <Show when={!props.compact}>
                  <span> · {d().song.bpm} BPM · {mmss(d().position)} / {mmss(d().length)}</span>
                </Show>
              </span>
            </span>
            <Show when={d().onAir} fallback={<span class="deck-chip is-cued">{d().playing ? "Playing" : "Cued"}</span>}>
              <span class="deck-chip">On air</span>
            </Show>
          </div>
        )}
      </Index>
    </div>
  );
}
