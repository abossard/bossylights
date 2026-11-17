import { createSignal, For, Match, Show, Switch } from "solid-js";
import { store, type Item } from "../../fake/store";
import { Button, Pad, Region, Segmented, type PadState } from "../../kit";
import type { Issue } from "../../shell/registry";
import { PatternEditor } from "./PatternEditor";
import { SequenceEditor } from "./SequenceEditor";
import { StepEditor } from "./StepEditor";
import { KIND_NAME, KindSymbol } from "./Symbol";
import "./looks.css";

export const id = "looks";
export const title = "Looks";
export const order = 3;
export const issues: Issue[] = [
  { number: 58, title: "Programmer: touch it, see it" },
  { number: 64, title: "Looks: store what you see" },
  { number: 63, title: "Palettes that stay linked" },
  { number: 59, title: "Effects across a group" },
  { number: 69, title: "Chasers and sequences" },
  { number: 60, title: "Pixel matrix effects" },
  { number: 62, title: "Audio-reactive color scripts (HUE Matrix)" },
  { number: 71, title: "Collections" },
];

const HOLD_MS = 350;
type Kind = "all" | Item["kind"];

export function Screen() {
  // ---- what is open: a path, so drilling in has a way back
  const [path, setPath] = createSignal<string[]>(["build-up-end-a"]);
  const current = () => store.item(path()[path().length - 1])!;
  const open = (id: string) => setPath([id]);
  const drill = (id: string) => setPath([...path(), id]);
  const [selectedEntry, setSelectedEntry] = createSignal<string | null>(null);
  const entryOf = () => {
    const c = current();
    if (c.kind !== "sequence") return null;
    return selectedEntry() && c.entries.some((x) => x.id === selectedEntry()) ? selectedEntry() : (c.entries[0]?.id ?? null);
  };

  // ---- library filters
  const [query, setQuery] = createSignal("");
  const [kind, setKind] = createSignal<Kind>("all");
  const [group, setGroup] = createSignal<string | null>(null);
  const shown = () =>
    store
      .items()
      .filter((i) => kind() === "all" || i.kind === kind())
      .filter((i) => !group() || i.groups.includes(group()!))
      .filter((i) => !query().trim() || i.name.toLowerCase().includes(query().trim().toLowerCase()))
      .sort((a, b) => (a.kind === b.kind ? a.name.localeCompare(b.name) : ["sequence", "pattern", "step"].indexOf(a.kind) - ["sequence", "pattern", "step"].indexOf(b.kind)));

  // ---- try on lights: tap toggles, hold flashes. A try gives back what it touched when it stops.
  let wasRunning = false;
  const tryDown = () => {
    const s = store.lookState(current().id);
    wasRunning = s === "held" || s === "flash";
    if (!wasRunning) store.hold(current().id);
  };
  const tryUp = (ms: number) => {
    const id = current().id;
    if (wasRunning) {
      if (ms < HOLD_MS) store.stop(id, true);
    } else if (ms < HOLD_MS) store.start(id);
    else store.release(id);
  };
  const tryState = (): PadState => store.lookState(current().id) ?? "idle";

  const [newGroup, setNewGroup] = createSignal("");
  const [consoleMsg, setConsoleMsg] = createSignal("");

  return (
    <div class="lk-screen">
      <Region id="library" title="Library" class="lk-lib" aside={<span class="mute">{shown().length} looks</span>}>
        <input type="text" placeholder="Search" aria-label="Search the library" value={query()} onInput={(e) => setQuery(e.currentTarget.value)} />
        <Segmented
          label="Kind"
          value={kind()}
          onChange={setKind}
          options={[
            { value: "all", label: "All" },
            { value: "step", label: "Step" },
            { value: "sequence", label: "Sequence" },
            { value: "pattern", label: "Pattern" },
          ]}
          class="lk-kinds"
        />
        <div class="lk-groups" role="group" aria-label="Groups">
          <For each={store.allGroups()}>
            {(g) => (
              <button type="button" class="lk-group" aria-pressed={group() === g} onClick={() => setGroup(group() === g ? null : g)}>
                {g}
              </button>
            )}
          </For>
        </div>
        <ul class="lk-list" aria-label="Looks">
          <For each={shown()} fallback={<li class="mute lk-none">Nothing matches. Clear the search or the filters.</li>}>
            {(i) => (
              <li>
                <button type="button" class="lk-row" aria-current={current().id === i.id ? "true" : undefined} onClick={() => open(i.id)}>
                  <KindSymbol kind={i.kind} />
                  <span class="lk-row-text">{i.name}</span>
                  <span class="lk-chips is-small">
                    <For each={store.colorsOf(i.id).slice(0, 4)}>{(c) => <i style={{ background: c }} />}</For>
                  </span>
                  <span class="lk-meas">{store.measureOf(i.id) ? `${store.measureOf(i.id)} b` : ""}</span>
                </button>
              </li>
            )}
          </For>
        </ul>
      </Region>

      <Region id="editor" title="Editor" bare class="lk-editor">
        <nav class="lk-crumbs" aria-label="Where you are">
          <For each={path()}>
            {(id, i) => (
              <>
                <Show when={i() > 0}>
                  <svg width="10" height="12" viewBox="0 0 10 12" aria-hidden="true" class="lk-crumb-sep">
                    <path d="M2 1l6 5-6 5" fill="none" stroke="currentColor" stroke-width="2" />
                  </svg>
                </Show>
                <Show when={i() < path().length - 1} fallback={<span class="lk-crumb is-here">{store.item(id)?.name}</span>}>
                  <button type="button" class="lk-crumb" onClick={() => setPath(path().slice(0, i() + 1))}>
                    {store.item(id)?.name}
                  </button>
                </Show>
              </>
            )}
          </For>
        </nav>
        <header class="lk-head">
          <KindSymbol kind={current().kind} size={24} />
          <h2 class="display">{current().name}</h2>
          <span class="lk-kind">{KIND_NAME[current().kind]}</span>
          <Show when={store.measureOf(current().id)}>
            <span class="mute">{store.measureOf(current().id)} beats per loop</span>
          </Show>
          <Show when={path().length > 1}>
            <Button class="lk-back" onClick={() => setPath(path().slice(0, -1))}>
              Back to {store.item(path()[path().length - 2])?.name}
            </Button>
          </Show>
        </header>
        <Switch>
          <Match when={current().kind === "sequence" && current()}>
            {(c) => (
              <SequenceEditor
                seq={c() as Extract<Item, { kind: "sequence" }>}
                selected={entryOf()}
                onSelect={setSelectedEntry}
                onOpen={(ref) => {
                  setSelectedEntry(null);
                  drill(ref);
                }}
              />
            )}
          </Match>
          <Match when={current().kind === "step" && current()}>{(c) => <StepEditor step={c() as Extract<Item, { kind: "step" }>} />}</Match>
          <Match when={current().kind === "pattern" && current()}>{(c) => <PatternEditor pattern={c() as Extract<Item, { kind: "pattern" }>} />}</Match>
        </Switch>
      </Region>

      <Region id="about" title="About this look" class="lk-about">
        <div class="lk-try">
          <Pad label="Try on lights" colors={store.colorsOf(current().id).length ? store.colorsOf(current().id) : ["#000000"]} state={tryState()} onDown={tryDown} onUp={tryUp} />
          <p class="mute">Tap to play it by hand, tap again to stop. Hold to flash.</p>
        </div>
        <div class="lk-console">
          <Button
            onClick={() => {
              const n = store.putOnConsole(current().id);
              setConsoleMsg(`On the Console, Main page, pad ${n}.`);
            }}
          >
            Put on Console
          </Button>
          <span class="mute" role="status">
            {consoleMsg() || (store.state.pads.main.includes(current().id) ? `On the Console, Main page, pad ${store.state.pads.main.indexOf(current().id) + 1}.` : "")}
          </span>
        </div>

        <section class="lk-about-sec">
          <h3 class="lk-label">Used in</h3>
          <Show when={store.usedIn(current().id).length} fallback={<p class="mute">No sequence uses it yet.</p>}>
            <p class="mute">A change here changes all of them.</p>
            <ul class="lk-used">
              <For each={store.usedIn(current().id)}>
                {(s) => (
                  <li>
                    <button type="button" class="lk-row" onClick={() => open(s.id)}>
                      <KindSymbol kind="sequence" />
                      <span class="lk-row-text">{s.name}</span>
                      <span class="lk-meas">{store.measureOf(s.id)} b</span>
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </section>

        <section class="lk-about-sec">
          <h3 class="lk-label">Groups</h3>
          <div class="lk-tags">
            <For each={current().groups} fallback={<span class="mute">In no group.</span>}>
              {(g) => (
                <span class="lk-tag">
                  {g}
                  <button type="button" aria-label={`Remove from ${g}`} onClick={() => store.removeFromGroup(current().id, g)}>
                    <svg width="10" height="10" viewBox="0 0 12 12" aria-hidden="true">
                      <path d="M2 2l8 8M10 2l-8 8" stroke="currentColor" stroke-width="2.5" />
                    </svg>
                  </button>
                </span>
              )}
            </For>
          </div>
          <form
            class="lk-addgroup"
            onSubmit={(e) => {
              e.preventDefault();
              store.addToGroup(current().id, newGroup());
              setNewGroup("");
            }}
          >
            <input type="text" list="lk-group-names" placeholder="Add to a group" aria-label="Add to a group" value={newGroup()} onInput={(e) => setNewGroup(e.currentTarget.value)} />
            <datalist id="lk-group-names">
              <For each={store.allGroups().filter((g) => !current().groups.includes(g))}>{(g) => <option value={g} />}</For>
            </datalist>
            <Button type="submit" disabled={!newGroup().trim()} reason="Type a name">
              Add
            </Button>
          </form>
        </section>
      </Region>
    </div>
  );
}
