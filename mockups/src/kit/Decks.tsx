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
    <svg class="deck-icon" width="12" height="12" viewBox="0 0 14 14" aria-hidden="true">
      <Show when={props.playing} fallback={<path d="M3 2h3v10H3zM8 2h3v10H8z" fill="currentColor" />}>
        <path d="M3 1.5l9 5.5-9 5.5z" fill="currentColor" />
      </Show>
    </svg>
  );
}

/** A deck's state in a word. */
const stateWord = (d: Deck) => (!d.song ? "Stopped" : d.playing ? "Playing" : "Cued");

/**
 * The two VirtualDJ decks, numbered 1 and 2: state in a word (Playing, Cued,
 * Stopped), song title and artist, tempo, position. The deck that plays gets the
 * room and the largest title; the other one is as wide as its text needs.
 */
export function Decks(props: DecksProps) {
  return (
    <div class={`decks ${props.compact ? "decks-compact" : ""} ${props.class ?? ""}`} aria-label="Decks">
      <Index each={props.decks}>
        {(d) => (
          <div
            class="deck"
            classList={{ "is-onair": d().playing, "is-empty": !d().song }}
            aria-label={
              d().song
                ? `Deck ${d().number}, ${stateWord(d()).toLowerCase()}: ${d().song!.title} by ${d().song!.artist}`
                : `Deck ${d().number}: nothing loaded`
            }
          >
            <span class="deck-letter display">{d().number}</span>
            <span class="deck-song">
              <b class="deck-title display">{d().song?.title ?? "Nothing loaded"}</b>
              <span class="deck-meta">
                <span class="deck-state" classList={{ "is-playing": d().playing }}>{stateWord(d())}</span>
                <Show when={d().song} fallback={<span class="deck-detail">Load a song in VirtualDJ</span>}>
                  {(song) => (
                    <>
                      <PlayIcon playing={d().playing} />
                      <span class="deck-detail">
                        {song().artist}
                        <Show when={!props.compact}>
                          {" "}
                          · {song().bpm} BPM · {mmss(d().position)} / {mmss(d().length)}
                        </Show>
                      </span>
                    </>
                  )}
                </Show>
              </span>
            </span>
            <Show when={d().song && !props.compact}>
              <i class="deck-progress" style={{ width: `${Math.min(100, (d().position / d().length) * 100)}%` }} />
            </Show>
          </div>
        )}
      </Index>
    </div>
  );
}
