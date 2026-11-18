import { For } from "solid-js";
import { store } from "../../fake/store";

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
        onChange={(e) => store.setAutopilot("style", e.currentTarget.value)}
      >
        <For each={store.state.autopilot.styles}>{(s) => <option value={s.id}>{s.name}</option>}</For>
      </select>
    </>
  );
}
