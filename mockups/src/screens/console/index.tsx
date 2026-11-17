import { createEffect, createSignal, onCleanup, Show } from "solid-js";
import { groupNames, hhmm, store } from "../../fake/store";
import { Button, Decks, Region, Tabs, Timetable, stepWindow } from "../../kit";
import type { Issue } from "../../shell/registry";
import { ConsoleGrid, createConsole, WidgetDrawer } from "./Console";
import { AutoLaneControls } from "./AutoLaneControls";
import "./console.css";

export const id = "console";
export const title = "Console";
export const order = 1;
export const nowStrip = false;
export const issues: Issue[] = [
  { number: 74, title: "Build your own control surface" },
  { number: 80, title: "Widget set" },
  { number: 91, title: "Surface generated from your rig" },
  { number: 82, title: "Pads mirror the screen" },
  { number: 72, title: "Layers: base, override, flash" },
  { number: 73, title: "Launch on the beat" },
];

function OverrideNotice() {
  const first = () => store.overrides()[0];
  const groups = () => [...new Set(store.overrides().flatMap((o) => o.groups))];
  return (
    <Region id="override" title="Override notice" bare class="notice-region">
      <Show
        when={first()}
        fallback={
          <div class="notice is-calm" role="status">
            <span class="notice-sw" />
            <span>
              <strong>Manual is not overriding anything.</strong>{" "}
              <span class="mute">
                {store.autopilotNow().length
                  ? "Autopilot runs every group."
                  : store.state.autopilot.on && !store.state.night.live
                    ? "Autopilot waits for LIVE."
                    : "Autopilot is not playing."}
              </span>
            </span>
          </div>
        }
      >
        {(o) => (
          <div class="notice" role="status">
            <span class="notice-sw" />
            <span class="notice-text">
              <strong>Manual overrides {o().under} on {groupNames(groups())}</strong>{" "}
              <span class="mute">
                {store.overrides().map((x) => x.look.name).join(", ")} since {hhmm(o().block.start)}
              </span>
            </span>
            <Button variant="layer" layer="manual" size="lg" pressed class="notice-release" onClick={() => store.releaseAll()}>
              Release
            </Button>
          </div>
        )}
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
  const win = () => stepWindow(store.now(), narrow() ? 5 * 60 : 7 * 60, (narrow() ? 15 : 25) * 60, 5 * 60);

  const pageTabs = () => [
    ...c.pages.map((p) => ({ id: p.id, label: p.name })),
    { id: "__add", label: <PlusIcon />, ariaLabel: "Add page" },
  ];

  return (
    <div class="console-screen" classList={{ "is-editing": c.editing() }}>
      <div class="console-left">
        <Region id="decks" title="Decks" bare>
          <Decks decks={store.decks()} />
        </Region>
        <Region id="timetable" title="Layers" bare class="console-tt">
          <Timetable
            from={win().from}
            to={win().to}
            now={store.now()}
            lanes={store.lanes(win().from, win().to)}
            tickEvery={5 * 60}
            format={hhmm}
            laneHeight="minmax(76px, 1fr)"
            labelWidth="var(--console-label)"
            laneControls={(lane) => (lane.layer === "autopilot" ? <AutoLaneControls /> : undefined)}
          />
          <div class="console-tt-foot">
            <Button onClick={() => store.addLayer()}>
              <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
                <path d="M6 1v10M1 6h10" stroke="currentColor" stroke-width="2.5" />
              </svg>
              Add layer
            </Button>
            <span class="mute">The top layer that speaks wins.</span>
          </div>
        </Region>
        <Show when={c.editing()}>
          <WidgetDrawer model={c} />
        </Show>
      </div>

      <div class="console-right">
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
        <OverrideNotice />
        <ConsoleGrid model={c} />
      </div>
    </div>
  );
}
