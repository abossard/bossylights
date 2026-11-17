// The LIVE switch and its four indicators. Self-contained so it can be
// replaced in one place: the shell only renders <LiveSwitch />.
import { For, Show } from "solid-js";
import { INDICATOR_NAME, store, type Indicator } from "../fake/store";
import { go } from "./router";
import "./live-switch.css";

const SHORT: Record<Indicator, string> = { dj: "DJ", record: "Rec", autopilot: "Auto", lock: "Lock" };
const ORDER: Indicator[] = ["dj", "record", "autopilot", "lock"];

/** LIVE on: the night is running (performing). LIVE off: preparing. */
export function LiveSwitch() {
  const live = () => store.state.night.live;

  const state = (i: Indicator) =>
    !store.state.night.indicators[i] ? "off" : live() ? "active" : "armed";
  const stateText = (i: Indicator) =>
    ({ active: "on", armed: "on when LIVE", off: "turned off" })[state(i)];

  return (
    <div class="ls" role="group" aria-label="LIVE">
      <button
        type="button"
        role="switch"
        class="ls-switch"
        aria-checked={live()}
        aria-label={live() ? "LIVE is on: performing" : "LIVE is off: preparing"}
        onClick={() => store.setLive(!live())}
      >
        <span class="ls-track" aria-hidden="true">
          <span class="ls-knob" />
        </span>
        <span class="ls-word display">Live</span>
      </button>
      <div class="ls-inds">
        <For each={ORDER}>
          {(i) => (
            <button
              type="button"
              class={`ls-ind is-${state(i)}`}
              aria-pressed={store.state.night.indicators[i]}
              aria-label={`${INDICATOR_NAME[i]}: ${stateText(i)}`}
              title={`${INDICATOR_NAME[i]}: ${stateText(i)}`}
              onClick={() => store.toggleIndicator(i)}
            >
              {SHORT[i]}
            </button>
          )}
        </For>
      </div>
      <Show when={!store.state.lightsConnected}>
        <button type="button" class="ls-nolights" onClick={() => go("inout")}>
          No lights connected
        </button>
      </Show>
    </div>
  );
}
