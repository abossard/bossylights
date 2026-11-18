import { For } from "solid-js";
import type { Tone } from "./common.ts";

export interface NowLane {
  name: string;
  tone: Tone;
  /** What runs in this row now. Empty: nothing. */
  runs: readonly string[];
}

export interface NowStripProps {
  lanes: readonly NowLane[];
  onPress: () => void;
}

/** The three rows in one line, on every screen without its own time line. A press opens the Console. */
export function NowStrip(props: NowStripProps) {
  const words = () => props.lanes.map((l) => `${l.name}: ${l.runs.join(", ") || "nothing"}`).join(". ");
  return (
    <button type="button" class="k-now" aria-label={`Now strip. ${words()}. Opens the Console`} onClick={() => props.onPress()}>
      <span class="k-now-flag">NOW</span>
      <For each={props.lanes}>
        {(lane) => (
          <span class="k-now-lane" data-tone={lane.tone}>
            <span class="k-now-name">{lane.name}</span>
            <For each={lane.runs} fallback={<span class="k-now-empty">nothing</span>}>
              {(run) => <span class="k-now-block">{run}</span>}
            </For>
          </span>
        )}
      </For>
    </button>
  );
}
