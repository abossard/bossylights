// A light and a pixel panel after they were added: name, mode, group, and Remove.
// Review 4 (issue #102, decision 29): no mockup had a control for these. They show the controls and say
// what would happen: the fake patch is not changed.
import { createSignal, For, Show } from "solid-js";
import { Button } from "../../kit";

const GROUPS = ["Movers", "PARs", "Bars", "Laser"];

/** The selected light: its name, its mode, its group, and Remove. */
export function LightSetup(props: { name: string; mode: string; group: string }) {
  const [said, setSaid] = createSignal("");
  return (
    <div class="tools tools-col" role="group" aria-label="This light">
      <label class="tools-field">
        Name
        <input type="text" aria-label="Name of the light" value={props.name} onChange={(e) => setSaid(`The light is called ${e.currentTarget.value || props.name}`)} />
      </label>
      <label class="tools-field">
        Mode
        <select aria-label="Mode of the light" onChange={(e) => setSaid(`${e.currentTarget.value}: the light needs other channels. If they do not fit here, it says so and nothing changes`)}>
          <For each={[props.mode, "Basic, 4 channels", "Extended, 16 channels"]}>{(mode) => <option>{mode}</option>}</For>
        </select>
      </label>
      <label class="tools-field">
        Group
        <select aria-label="Group of the light" value={props.group} onChange={(e) => setSaid(`The light is the last of ${e.currentTarget.value}`)}>
          <For each={GROUPS}>{(group) => <option>{group}</option>}</For>
        </select>
      </label>
      <Button aria-label="Remove the light" onClick={() => setSaid("The light leaves the patch. Looks that set it keep their values for it. Undo puts it back")}>
        Remove light
      </Button>
      <Show when={said()}>
        <span class="mute" role="status">
          {said()}
        </span>
      </Show>
    </div>
  );
}

/** The selected pixel panel: change it in the form it was made with, or remove it. */
export function PanelSetup(props: { name: string; onChange: () => void }) {
  const [said, setSaid] = createSignal("");
  return (
    <div class="tools" role="group" aria-label="This panel">
      <Button aria-label="Change the panel" onClick={() => props.onChange()}>
        Change panel
      </Button>
      <Button aria-label="Remove the panel" onClick={() => setSaid(`${props.name} leaves the patch and frees its universes. Undo puts it back`)}>
        Remove panel
      </Button>
      <Show when={said()}>
        <span class="mute" role="status">
          {said()}
        </span>
      </Show>
    </div>
  );
}
