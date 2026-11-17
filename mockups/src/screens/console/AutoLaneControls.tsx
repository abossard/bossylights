import { For } from "solid-js";
import { store, type AutopilotStyle } from "../../fake/store";

const STYLES: { value: AutopilotStyle; label: string }[] = [
  { value: "chill", label: "Chill" },
  { value: "club", label: "Club" },
  { value: "peak", label: "Peak" },
  { value: "no-strobe", label: "No strobe" },
];

/** The compact autopilot header in the Console timetable: on/off and style. */
export function AutoLaneControls() {
  return (
    <>
      <button
        type="button"
        class="auto-toggle"
        aria-pressed={store.state.autopilot.on}
        aria-label={`Autopilot ${store.state.autopilot.on ? "on" : "off"}`}
        onClick={() => store.setAutopilot("on", !store.state.autopilot.on)}
      >
        {store.state.autopilot.on ? "On" : "Off"}
      </button>
      <select
        aria-label="Autopilot style"
        value={store.state.autopilot.style}
        onChange={(e) => store.setAutopilot("style", e.currentTarget.value as AutopilotStyle)}
      >
        <For each={STYLES}>{(s) => <option value={s.value}>{s.label}</option>}</For>
      </select>
    </>
  );
}
