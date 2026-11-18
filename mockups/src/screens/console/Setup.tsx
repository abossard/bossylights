// What a page and a widget are, set while the surface is edited. Review 4 (issue #102, decision 29):
// no mockup had a control for these. The values live here: this mockup shows the controls, not a saved layout.
import { createSignal, For, Show } from "solid-js";
import { store } from "../../fake/store";
import { Button } from "../../kit";
import type { ConsoleModel } from "./Console";

const FADERS = ["Master", "Movers", "PARs", "Bars", "Laser", "Speed"];

/** The page that is open: its name, its place among the tabs, and Delete. */
export function PageSetup(props: { model: ConsoleModel }) {
  const m = props.model;
  const [said, setSaid] = createSignal("");
  const only = () => m.pages.length < 2;
  return (
    <section class="tools" aria-label="This page">
      <label class="tools-field">
        Name of the page
        <input type="text" aria-label="Name of the page" value={m.page().name} onChange={(e) => setSaid(`The page is called ${e.currentTarget.value || m.page().name}`)} />
      </label>
      <Button aria-label="Move the page earlier" disabled={m.pageIndex() === 0} reason="It is the first" onClick={() => setSaid("The page moves one tab to the left")}>
        Earlier
      </Button>
      <Button aria-label="Move the page later" disabled={m.pageIndex() === m.pages.length - 1} reason="It is the last" onClick={() => setSaid("The page moves one tab to the right")}>
        Later
      </Button>
      <Button aria-label="Delete the page" disabled={only()} reason="The last page stays" onClick={() => setSaid("The page and its widgets are gone. Undo puts them back")}>
        Delete page
      </Button>
      <Show when={said()}>
        <span class="mute" role="status">
          {said()}
        </span>
      </Show>
    </section>
  );
}

/** What the widgets of the page play, move and say. One row per widget that has something to set. */
export function WidgetSetup(props: { model: ConsoleModel }) {
  const looks = () => store.items().map((i) => i.name);
  const [cues, setCues] = createSignal(looks().slice(0, 4));
  const swap = (i: number, by: number) => {
    const next = [...cues()];
    [next[i], next[i + by]] = [next[i + by], next[i]];
    setCues(next);
  };
  return (
    <section class="tools tools-col" aria-label="What the widgets do">
      <For each={props.model.page().widgets}>
        {(w) => (
          <div class="tools">
            <b>{w.title}</b>
            <Show when={w.kind === "button"}>
              <label class="tools-field">
                Plays
                <select aria-label="Look this pad plays">
                  <For each={looks()}>{(name) => <option>{name}</option>}</For>
                </select>
              </label>
            </Show>
            <Show when={w.kind === "slider"}>
              <label class="tools-field">
                Moves
                <select aria-label="What this slider moves">
                  <For each={FADERS}>{(name) => <option>{name}</option>}</For>
                </select>
              </label>
            </Show>
            <Show when={w.kind === "label" || w.kind === "frame"}>
              <label class="tools-field">
                Words
                <input type="text" aria-label="Words of the label or frame" value={w.title} />
              </label>
            </Show>
            <Show when={w.kind === "cuelist"}>
              <ol class="tools-list" aria-label="Looks of the cue list, in order">
                <For each={cues()}>
                  {(name, i) => (
                    <li>
                      <span>{name}</span>
                      <Show when={i() > 0}>
                        <Button aria-label="Move the cue earlier" onClick={() => swap(i(), -1)}>
                          Earlier
                        </Button>
                      </Show>
                      <Show when={i() < cues().length - 1}>
                        <Button aria-label="Move the cue later" onClick={() => swap(i(), 1)}>
                          Later
                        </Button>
                      </Show>
                      <Button aria-label="Take the cue out" onClick={() => setCues(cues().filter((_, at) => at !== i()))}>
                        Take out
                      </Button>
                    </li>
                  )}
                </For>
              </ol>
              <label class="tools-field">
                Add a look
                <select aria-label="Add a look to the cue list" onChange={(e) => (setCues([...cues(), e.currentTarget.value]), (e.currentTarget.selectedIndex = 0))}>
                  <option>Pick a look</option>
                  <For each={looks()}>{(name) => <option>{name}</option>}</For>
                </select>
              </label>
            </Show>
          </div>
        )}
      </For>
    </section>
  );
}
