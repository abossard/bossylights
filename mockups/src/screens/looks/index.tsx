import { createSignal, For, Match, Show, Switch } from "solid-js";
import { store, type Item } from "../../fake/store";
import { Button, Pad, Region, Segmented, type PadState } from "../../kit";
import type { Issue } from "../../shell/registry";
import { Autopilot } from "./Autopilot";
import { Connections } from "./Connections";
import { Library } from "./Library";
import { PatternEditor } from "./PatternEditor";
import { SequenceEditor } from "./SequenceEditor";
import { StepEditor } from "./StepEditor";
import { KIND_NAME, KindSymbol } from "./Symbol";
import "./looks.css";
import { LookTools } from "./Manage";

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
  { number: 87, title: "Autopilot: the show follows the music" },
  { number: 86, title: "Never-dark fallback" },
];

const HOLD_MS = 350;

export function Screen() {
  // ---- what is open: a path, so drilling in has a way back
  // a jump from another screen can land on a look or on an autopilot rule
  const jump = store.state.jump;
  const [part, setPart] = createSignal<"looks" | "autopilot">(jump.autopilotRule ? "autopilot" : "looks");
  const [apTarget, setApTarget] = createSignal<{ styleId?: string; ruleId?: string }>({ ruleId: jump.autopilotRule });
  const [wide, setWide] = createSignal(false);
  const [path, setPath] = createSignal<string[]>([jump.look && store.item(jump.look) ? jump.look : "build-up-end-a"]);
  const current = () => store.item(path()[path().length - 1])!;
  const open = (id: string) => (setPath([id]), setWide(false));
  const drill = (id: string) => setPath([...path(), id]);
  const [selectedEntry, setSelectedEntry] = createSignal<string | null>(null);
  const entryOf = () => {
    const c = current();
    if (c.kind !== "sequence") return null;
    return selectedEntry() && c.entries.some((x) => x.id === selectedEntry()) ? selectedEntry() : (c.entries[0]?.id ?? null);
  };

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

  const [consoleMsg, setConsoleMsg] = createSignal("");

  return (
    <div class="lk-screen">
      <div class="lk-parts">
        <Segmented
          label="Part of the Looks screen"
          value={part()}
          onChange={setPart}
          options={[
            { value: "looks", label: "Looks" },
            { value: "autopilot", label: "Autopilot" },
          ]}
        />
        <span class="mute">{part() === "looks" ? "What exists: steps, sequences and patterns, in groups" : "How the autopilot picks from those groups"}</span>
      </div>
      <Show when={part() === "looks"} fallback={<Autopilot styleId={apTarget().styleId} ruleId={apTarget().ruleId} />}>
      <div class="lk-body" classList={{ "is-wide": wide() }}>
      <Library currentId={current().id} onOpen={open} wide={wide()} onWide={setWide} />

      <Show when={!wide()}>
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
        <LookTools id={current().id} />
        <div class="lk-tags" role="group" aria-label="Groups this look is in">
          <span class="lk-label">In groups</span>
          <For each={current().groups} fallback={<span class="mute">none</span>}>
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
          <select
            class="lk-tag-add"
            aria-label="Add to a group"
            value=""
            onChange={(e) => {
              if (e.currentTarget.value) store.addToGroup(current().id, e.currentTarget.value);
              e.currentTarget.value = "";
            }}
          >
            <option value="">Add to a group</option>
            <For each={store.allGroups().filter((g) => !current().groups.includes(g))}>{(g) => <option value={g}>{g}</option>}</For>
          </select>
        </div>
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

      <div class="lk-side">
      <Region id="about" title="Try it" class="lk-about">
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

      </Region>
      <Connections
        id={current().id}
        onOpen={open}
        onRule={(styleId, ruleId) => {
          setApTarget({ styleId, ruleId });
          setPart("autopilot");
        }}
      />
      </div>
      </Show>
      </div>
      </Show>
    </div>
  );
}
