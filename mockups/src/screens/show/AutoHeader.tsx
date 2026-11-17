import { For } from "solid-js";
import { store, type AutopilotStyle } from "../../fake/store";
import { Fader } from "../../kit";

const STYLES: { value: AutopilotStyle; label: string }[] = [
  { value: "chill", label: "Chill" },
  { value: "club", label: "Club" },
  { value: "peak", label: "Peak" },
  { value: "no-strobe", label: "No strobe" },
];

const wildWord = (v: number) => (v < 25 ? "Calm" : v < 50 ? "Easy" : v < 75 ? "Lively" : "Wild");

/** The Auto lane header: everything the autopilot is told, in one place. */
export function AutoHeader() {
  const ap = () => store.state.autopilot;
  return (
    <div class="show-auto-head layer-autopilot">
      <div class="show-auto-row">
        <h3 class="display">Auto</h3>
        <button
          type="button"
          class="show-auto-toggle"
          aria-pressed={ap().on}
          aria-label={`Autopilot ${ap().on ? "on" : "off"}`}
          onClick={() => store.setAutopilot("on", !ap().on)}
        >
          {ap().on ? "ON" : "OFF"}
        </button>
        <select
          aria-label="Autopilot style"
          value={ap().style}
          onChange={(e) => store.setAutopilot("style", e.currentTarget.value as AutopilotStyle)}
        >
          <For each={STYLES}>{(s) => <option value={s.value}>{s.label}</option>}</For>
        </select>
      </div>
      <div class="show-auto-wild">
        <span>How wild</span>
        <b>{wildWord(ap().wild)}</b>
        <Fader
          orientation="horizontal"
          bare
          layer="autopilot"
          label="How wild"
          valueText={wildWord(ap().wild)}
          value={ap().wild}
          onChange={(v) => store.setAutopilot("wild", v)}
        />
      </div>
      <div class="show-auto-groups" role="group" aria-label="Groups it may use">
        <For each={store.groups}>
          {(g) => (
            <button type="button" aria-pressed={ap().groups[g.id]} onClick={() => store.toggleGroup(g.id)}>
              {g.name}
            </button>
          )}
        </For>
      </div>
    </div>
  );
}
