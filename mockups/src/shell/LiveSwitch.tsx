// The LIVE switch and its four indicators. Self-contained so it can be
// replaced in one place: the shell only renders <LiveSwitch />.
import { For, Show } from "solid-js";
import { INDICATOR_NAME, store, type Indicator } from "../fake/store";
import { go } from "./router";
import "./live-switch.css";

const ORDER: Indicator[] = ["dj", "record", "autopilot", "lock"];

/** What each indicator does while LIVE is on, in plain words. */
const DOES: Record<Indicator, string> = {
  dj: "follows VirtualDJ",
  record: "records the set",
  autopilot: "the autopilot plays",
  lock: "the surface is locked",
};

/** LIVE on: the night is running (performing). LIVE off: preparing. */
export function LiveSwitch() {
  const live = () => store.state.night.live;

  /** on: in effect now. ready: will be in effect when LIVE turns on. off: turned off. */
  const state = (i: Indicator) => (!store.state.night.indicators[i] ? "off" : live() ? "on" : "ready");
  const stateText = (i: Indicator) =>
    ({ on: `on, ${DOES[i]}`, ready: "ready, starts with LIVE", off: "turned off" })[state(i)];

  return (
    <div class="ls" role="group" aria-label="LIVE">
      <button
        type="button"
        role="switch"
        class="ls-switch"
        aria-checked={live()}
        aria-label={live() ? "LIVE is on: the night is running" : "LIVE is off: preparing"}
        onClick={() => store.setLive(!live())}
      >
        <span class="ls-track" aria-hidden="true">
          <span class="ls-knob" />
        </span>
        <span class="ls-word display">LIVE</span>
        <span class="ls-state">{live() ? "on" : "off"}</span>
      </button>
      <div class="ls-inds">
        <For each={ORDER}>
          {(i) => (
            <button
              type="button"
              class={`ls-ind is-${state(i)} ind-${i}`}
              aria-pressed={store.state.night.indicators[i]}
              aria-label={`${INDICATOR_NAME[i]}: ${stateText(i)}`}
              title={`${INDICATOR_NAME[i]}: ${stateText(i)}`}
              onClick={() => store.toggleIndicator(i)}
            >
              <i class="ls-lamp" aria-hidden="true" />
              {INDICATOR_NAME[i]}
            </button>
          )}
        </For>
      </div>
      <Show when={!store.state.lightsConnected}>
        <button type="button" class="ls-nolights" onClick={() => go("inout")}>
          <i class="ls-lamp" aria-hidden="true" />
          No lights<span class="ls-long"> connected</span>
        </button>
      </Show>
    </div>
  );
}
