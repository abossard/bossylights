import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";
import { hhmm, store, type Lane } from "../../fake/store";
import { Button, Decks, Region, Tabs, Timetable, stepWindow } from "../../kit";
import type { Issue } from "../../shell/registry";
import { ConsoleGrid, createConsole, WidgetDrawer } from "./Console";
import { AutoLaneControls } from "./AutoLaneControls";
import "./console.css";
import { PageSetup, WidgetSetup } from "./Setup";

export const id = "console";
export const title = "Console";
export const order = 1;
export const nowStrip = false;
export const issues: Issue[] = [
  { number: 74, title: "Build your own control surface" },
  { number: 80, title: "Widget set" },
  { number: 91, title: "Surface generated from your rig" },
  { number: 82, title: "Pads mirror the screen" },
  { number: 72, title: "Who wins: manual, show, autopilot" },
  { number: 73, title: "Launch on the beat" },
];

const FOLD_KEY = "bossylights.console.running.folded";
const readFolded = () => {
  try {
    return localStorage.getItem(FOLD_KEY) === "1";
  } catch {
    return false;
  }
};

/**
 * Running: the timetable and the list of what runs, in one panel. Every look
 * has its own button, the hold notice has a reserved line, and nothing in or
 * under the panel moves when a look starts or stops.
 */
function Running(props: { narrow: boolean }) {
  const [folded, setFolded] = createSignal(readFolded());
  const fold = (on: boolean) => {
    setFolded(on);
    try {
      localStorage.setItem(FOLD_KEY, on ? "1" : "0");
    } catch {
      // private window: it still folds, it is just not remembered
    }
  };
  const run = () => store.running();
  const held = () => store.holds();
  const win = () => stepWindow(store.now(), 5 * 60, (props.narrow ? 15 : 25) * 60, 5 * 60);

  const Items = (p: { rank: Lane["layer"] }) => (
    <div class="run-items">
      <Show when={p.rank === "autopilot"}>
        <AutoLaneControls />
      </Show>
      <For each={run().filter((r) => r.rank === p.rank)} fallback={<span class="run-none">{p.rank === "manual" ? "nothing held" : "nothing running"}</span>}>
        {(r) => (
          <span class="run-item" classList={{ "is-off": !!r.padOff }}>
            <i class="run-sw" style={{ background: r.colors[0] }} aria-hidden="true" />
            <b>{r.name}</b>
            <Show when={r.padOff}>
              <em>pad off</em>
            </Show>
            <button
              type="button"
              aria-label={`${p.rank === "manual" ? "Release" : "Stop"} ${r.name}`}
              onClick={() => (p.rank === "manual" ? store.releaseHold(r.id) : store.stopLook(r.lookId))}
            >
              {p.rank === "manual" ? "Release" : "Stop"}
            </button>
          </span>
        )}
      </For>
    </div>
  );

  return (
    <Region id="running" title="Running" bare class={`run ${folded() ? "is-folded" : ""}`}>
      <div class="run-head">
        <button type="button" class="run-fold" aria-expanded={!folded()} aria-label={folded() ? "Unfold Running" : "Fold Running"} onClick={() => fold(!folded())}>
          <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
            <path d="M2 4l4 4 4-4" fill="none" stroke="currentColor" stroke-width="2.5" />
          </svg>
          <span class="display">Running</span>
        </button>
        <div class="run-chips" role="list" aria-label="Running now">
          <Show when={folded()}>
            <For each={run()} fallback={<span class="run-none">nothing running</span>}>
              {(r) => (
                <span role="listitem" class={`run-chip layer-${r.rank}`} classList={{ "is-off": !!r.padOff }}>
                  {r.name}
                </span>
              )}
            </For>
          </Show>
        </div>
        <Decks decks={store.decks()} compact class="run-decks" />
        <span class="run-held" classList={{ "is-none": !held().length }}>
          <b>{held().length}</b> held
        </span>
        <Button variant="layer" layer="manual" class="run-release" disabled={!held().length} onClick={() => store.releaseAll()}>
          Release all
        </Button>
      </div>
      <Show when={!folded()}>
        <Timetable
          from={win().from}
          to={win().to}
          now={store.now()}
          lanes={store.lanes(win().from, win().to)}
          tickEvery={5 * 60}
          format={hhmm}
          laneHeight="54px"
          labelWidth="var(--run-label)"
          laneControls={(lane) => <Items rank={lane.layer} />}
        />
        {/* the hold notice: one reserved line, so showing it moves nothing */}
        <p class="run-notice" classList={{ "is-calm": !held().length }} role="status">
          <Show
            when={held().length}
            fallback={<>You hold nothing. {store.state.rules.manualEnds === "release" ? "What you touch stays yours until Release." : "What you touch is yours while its pad is on."}</>}
          >
            <strong>You hold, until Release:</strong>{" "}
            <For each={held()}>
              {(h, i) => (
                <span>
                  {i() ? " · " : ""}
                  <b>{h.name}</b> {store.holdText(h)}
                  {h.beaten.length ? ` (${h.beaten.join("; ")})` : ""}
                  {h.live ? "" : ", pad off"}
                </span>
              )}
            </For>
          </Show>
        </p>
      </Show>
    </Region>
  );
}

function PlusIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
      <path d="M8 1v14M1 8h14" stroke="currentColor" stroke-width="3" />
    </svg>
  );
}

export function Screen() {
  const c = createConsole();
  // Turning LIVE on locks the surface, so an edit in progress ends.
  createEffect(() => store.surfaceLocked() && c.setEditing(false));
  // The narrow layout shows 15 minutes so short blocks keep readable titles.
  const mq = matchMedia("(max-width: 1180px)");
  const [narrow, setNarrow] = createSignal(mq.matches);
  const onMq = () => setNarrow(mq.matches);
  mq.addEventListener("change", onMq);
  onCleanup(() => mq.removeEventListener("change", onMq));

  const pageTabs = () => [
    ...c.pages.map((p) => ({ id: p.id, label: p.name })),
    { id: "__add", label: <PlusIcon />, ariaLabel: "Add page" },
  ];

  return (
    <div class="console-screen" classList={{ "is-editing": c.editing() }}>
      <Running narrow={narrow()} />
      <Region id="pages" title="Console pages" bare class="console-pagebar">
        <Tabs
          label="Console pages"
          items={pageTabs()}
          value={c.page().id}
          onChange={(id) => (id === "__add" ? c.addPage() : c.setPageId(id))}
        />
        <div class="console-actions">
          <Show
            when={c.editing()}
            fallback={
              <>
                <Button onClick={() => store.surprise()}>Surprise me</Button>
                <Button
                  disabled={store.surfaceLocked()}
                  reason="Locked while LIVE is on"
                  onClick={() => (c.setMessage(""), c.setEditing(true))}
                >
                  Edit surface
                </Button>
              </>
            }
          >
            <Button variant="primary" onClick={() => c.setEditing(false)}>
              Done editing
            </Button>
          </Show>
        </div>
      </Region>
      <Show when={c.editing()}>
        <PageSetup model={c} />
        <WidgetDrawer model={c} />
        <WidgetSetup model={c} />
      </Show>
      <ConsoleGrid model={c} />
    </div>
  );
}
