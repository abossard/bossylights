import { createMemo, createSignal, For, Show } from "solid-js";
import { COLOUR_ORDERS, store, type ColourOrder, type Fixture, type PixelPanel } from "../../fake/store";
import { Button, Region, Segmented } from "../../kit";
import type { Issue } from "../../shell/registry";
import { AddFlow, flow, flowInProgress, ghost, openFlow, STEPS } from "./AddFlow";
import "./patch.css";

export const id = "patch";
export const title = "Patch";
export const order = 4;
export const issues: Issue[] = [
  { number: 50, title: "Patch: fixtures, modes, addresses" },
  { number: 54, title: "Guided patch" },
  { number: 41, title: "Fixture library: QLC+, OFL, GDTF" },
  { number: 56, title: "Groups and fixture order" },
];

const COLS = 32;
/** A run's colour by group: tells groups apart, never a layer colour. */
const GROUP_COLOUR: Record<string, string> = { movers: "#b9a7ff", pars: "#8fd6b0", bars: "#e8b089", laser: "#9fd3ff", panel: "#d8d2c4" };
const range = (f: Fixture) => `${f.address} to ${f.address + f.channels - 1}`;

/** A run of addresses cut into one piece per grid row. */
function segments(address: number, channels: number) {
  const out: { row: number; col: number; len: number; first: boolean }[] = [];
  let a = address - 1;
  let left = channels;
  while (left > 0 && a < 512) {
    const row = Math.floor(a / COLS);
    const col = a % COLS;
    const len = Math.min(left, COLS - col);
    out.push({ row, col, len, first: out.length === 0 });
    a += len;
    left -= len;
  }
  return out;
}

export function Screen() {
  const [universe, setUniverse] = createSignal(1);
  // a jump from the Looks screen ("Lights: PAR 1-8") lands on the first light of that group
  const jumped = store.rig.find((f) => f.group === store.state.jump.fixtureGroup);
  const [selectedId, setSelectedId] = createSignal<string>(jumped?.id ?? store.conflicts()[0]?.b.id ?? store.rig[0].id);
  const [panelId, setPanelId] = createSignal<string | null>(null);
  const [side, setSide] = createSignal<"details" | "panel">("details");
  const [query, setQuery] = createSignal("");
  const [open, setOpen] = createSignal<Record<string, boolean>>({});

  const selected = () => store.fixtureById(selectedId()) ?? null;
  const panel = () => store.state.panels.find((p) => p.id === panelId()) ?? null;
  const select = (id: string) => {
    setSelectedId(id);
    setPanelId(null);
    if (store.state.identify && store.state.identify !== id) store.setIdentify(null);
  };

  // ---- universes: runs of one pixel panel collapse into one row
  const rows = createMemo(() => {
    const q = query().trim().toLowerCase();
    const list = store.universes().filter((u) => !q || `universe ${u.universe} ${u.panel?.name ?? ""}`.toLowerCase().includes(q) || String(u.universe) === q);
    const out: ({ kind: "u"; u: (typeof list)[number]; inPanel: boolean } | { kind: "panel"; panel: PixelPanel; from: number; to: number })[] = [];
    const seen = new Set<string>();
    for (const u of list) {
      if (u.panel && !seen.has(u.panel.id)) {
        seen.add(u.panel.id);
        const runs = store.panelRuns(u.panel);
        out.push({ kind: "panel", panel: u.panel, from: runs[0].universe, to: runs[runs.length - 1].universe });
      }
      if (!u.panel || open()[u.panel.id]) out.push({ kind: "u", u, inPanel: !!u.panel });
    }
    return out;
  });

  // ---- the grid of the selected universe
  const fixtures = () => store.rig.filter((f) => f.universe === universe());
  const runsHere = () => store.allPanelRuns().filter((r) => r.universe === universe());
  const conflictsHere = () => store.conflicts().filter((c) => c.a.universe === universe());
  const usedHere = () => store.universes().find((u) => u.universe === universe())?.used ?? 0;
  let grid!: HTMLDivElement;

  const drag = (f: Fixture) => (e: PointerEvent) => {
    if (e.button > 0) return;
    select(f.id);
    const el = e.currentTarget as HTMLElement;
    try {
      el.setPointerCapture(e.pointerId);
    } catch {
      // synthetic pointer: drag still works
    }
    const r = grid.getBoundingClientRect();
    const cw = r.width / COLS;
    const ch = r.height / 16;
    const x0 = e.clientX;
    const y0 = e.clientY;
    const a0 = f.address;
    const move = (ev: PointerEvent) => {
      const d = Math.round((ev.clientX - x0) / cw) + Math.round((ev.clientY - y0) / ch) * COLS;
      store.setAddress(f.id, a0 + d);
    };
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  };
  const keyMove = (f: Fixture) => (e: KeyboardEvent) => {
    const d = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -COLS, ArrowDown: COLS }[e.key];
    if (!d) return;
    e.preventDefault();
    store.setAddress(f.id, f.address + d);
  };

  /** Drag a fixture up or down inside its group: one place per row height. */
  const dragOrder = (f: Fixture) => (e: PointerEvent) => {
    if (e.button > 0) return;
    e.preventDefault();
    select(f.id);
    const h = (e.currentTarget as HTMLElement).closest("li")!.offsetHeight;
    let y0 = e.clientY;
    const move = (ev: PointerEvent) => {
      const steps = Math.trunc((ev.clientY - y0) / h);
      if (!steps) return;
      store.reorderFixture(f.id, steps);
      y0 += steps * h;
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  };

  const clashOf = (id: string) => store.conflicts().find((c) => c.a.id === id || c.b.id === id);
  const other = (id: string) => {
    const c = clashOf(id);
    return c ? (c.a.id === id ? c.b : c.a) : null;
  };

  // ---- add a pixel panel
  const [pp, setPp] = createSignal({ name: "Pixel panel 2", cols: 16, rows: 8, wiring: "snake" as PixelPanel["wiring"], startUniverse: 0, pixel: "RGB" as PixelPanel["pixel"], order: "RGB" as ColourOrder });
  const ppStart = () => pp().startUniverse || store.firstFreeUniverse();
  const ppPerUniverse = () => (pp().pixel === "RGBW" ? 128 : 170);
  const ppUniverses = () => Math.ceil((pp().cols * pp().rows) / ppPerUniverse());

  return (
    <div class="patch-screen">
      <Region id="universes" title="Universes" class="patch-unis" aside={<span class="mute">{store.universes().length}</span>}>
        <input type="text" placeholder="Search universes" aria-label="Search universes" value={query()} onInput={(e) => setQuery(e.currentTarget.value)} />
        <ul class="patch-uni-list">
          <For each={rows()}>
            {(r) =>
              r.kind === "panel" ? (
                <li>
                  <button
                    type="button"
                    class="patch-uni is-panel"
                    aria-expanded={!!open()[r.panel.id]}
                    onClick={() => {
                      setOpen({ ...open(), [r.panel.id]: !open()[r.panel.id] });
                      setPanelId(r.panel.id);
                      setUniverse(r.from);
                    }}
                  >
                    <svg class="patch-chev" width="10" height="12" viewBox="0 0 10 12" aria-hidden="true">
                      <path d="M2 1l6 5-6 5" fill="none" stroke="currentColor" stroke-width="2.5" />
                    </svg>
                    <span class="patch-uni-text">
                      <b>
                        Universes {r.from} to {r.to}
                      </b>
                      <span class="mute">
                        {r.panel.name}, {r.panel.cols} x {r.panel.rows}
                      </span>
                    </span>
                  </button>
                </li>
              ) : (
                <li>
                  <button type="button" class="patch-uni" classList={{ "is-in-panel": r.inPanel }} aria-current={universe() === r.u.universe ? "true" : undefined} onClick={() => setUniverse(r.u.universe)}>
                    <span class="patch-uni-text">
                      <b>
                        Universe {r.u.universe}
                        <Show when={r.u.conflict}>
                          <i class="patch-clash-mark" aria-label="has a clash" />
                        </Show>
                      </b>
                      <span class="mute">{r.u.panel ? "pixels" : `${r.u.fixtures} lights`} · {r.u.used} of 512</span>
                      <span class="patch-fill" aria-hidden="true">
                        <i style={{ width: `${(r.u.used / 512) * 100}%` }} />
                      </span>
                    </span>
                  </button>
                </li>
              )
            }
          </For>
        </ul>
        <div class="patch-adds">
          <Show
            when={!flow.open}
            fallback={
              <p class="patch-adding">
                Adding a light, step {flow.step} of 4: {STEPS[flow.step - 1]}
              </p>
            }
          >
            <Button variant="primary" onClick={() => (setSide("details"), openFlow())}>
              {flowInProgress() ? `Continue adding (step ${flow.step} of 4)` : "Add lights"}
            </Button>
          </Show>
          <Button pressed={side() === "panel"} onClick={() => setSide(side() === "panel" ? "details" : "panel")}>
            Add a pixel panel
          </Button>
        </div>
      </Region>

      <div class="patch-centre">
        <Region id="grid" title="Universe grid" class="patch-grid-region" aside={<span class="mute">Universe {universe()} · {usedHere()} of 512 used · drag a light to move it</span>}>
          <div class="patch-grid-wrap">
            <div class="patch-rownums" aria-hidden="true">
              <For each={Array.from({ length: 16 }, (_, i) => i)}>{(i) => <span>{i * COLS + 1}</span>}</For>
            </div>
            <div ref={grid} class="patch-grid" role="group" aria-label={`The 512 addresses of universe ${universe()}`}>
              <For each={runsHere()}>
                {(r) => (
                  <For each={segments(r.address, r.channels)}>
                    {(s) => (
                      <button
                        type="button"
                        class="patch-run is-panel"
                        classList={{ "is-selected": panelId() === r.panel.id }}
                        style={{ "--row": s.row, "--col": s.col, "--len": s.len, "--c": GROUP_COLOUR.panel }}
                        onClick={() => setPanelId(r.panel.id)}
                        aria-label={`${r.panel.name}, pixels ${r.firstPixel} to ${r.lastPixel}`}
                      >
                        <Show when={s.first}>
                          <span>
                            {r.panel.name}, pixels {r.firstPixel} to {r.lastPixel}
                          </span>
                        </Show>
                      </button>
                    )}
                  </For>
                )}
              </For>
              <For each={fixtures()}>
                {(f) => (
                  <For each={segments(f.address, f.channels)}>
                    {(s) => (
                      <button
                        type="button"
                        class="patch-run"
                        classList={{ "is-selected": selectedId() === f.id && !panelId() }}
                        style={{ "--row": s.row, "--col": s.col, "--len": s.len, "--c": GROUP_COLOUR[f.group] }}
                        aria-label={`${f.name}, ${range(f)}. Drag or use arrow keys to give it another address.`}
                        onPointerDown={drag(f)}
                        onKeyDown={keyMove(f)}
                      >
                        <Show when={s.first}>
                          <span>{f.name}</span>
                        </Show>
                      </button>
                    )}
                  </For>
                )}
              </For>
              <Show when={ghost() && universe() === 1}>
                <For each={segments(ghost()!.address, ghost()!.channels)}>
                  {(s) => (
                    <span class="patch-run is-ghost" style={{ "--row": s.row, "--col": s.col, "--len": s.len }} aria-hidden="true">
                      <Show when={s.first}>
                        <span>{ghost()!.name}</span>
                      </Show>
                    </span>
                  )}
                </For>
              </Show>
              <For each={conflictsHere()}>
                {(c) => (
                  <For each={segments(c.from, c.to - c.from + 1)}>
                    {(s) => <span class="patch-clash" style={{ "--row": s.row, "--col": s.col, "--len": s.len }} aria-hidden="true" />}
                  </For>
                )}
              </For>
            </div>
          </div>
          <Show when={conflictsHere().length}>
            <p class="patch-clash-note">
              <i class="patch-clash-mark" aria-hidden="true" /> {conflictsHere().map((c) => `${c.a.name} and ${c.b.name} share ${c.from} to ${c.to}`).join(", ")}.
            </p>
          </Show>
        </Region>

        <Region id="fixtures" title="Fixtures" class="patch-fixtures" aside={<span class="mute">The order in a group is what "left to right" means</span>}>
          <div class="patch-groups">
            <For each={store.groups}>
              {(g) => (
                <section>
                  <h3 class="patch-group-h">{g.name}</h3>
                  <ol>
                    <For each={store.rig.filter((f) => f.group === g.id)}>
                      {(f, i) => (
                        <li class="patch-fx" classList={{ "is-selected": selectedId() === f.id && !panelId() }}>
                          <span class="patch-order" title="Drag to change the order" onPointerDown={dragOrder(f)}>
                            <svg width="8" height="14" viewBox="0 0 8 14" aria-hidden="true">
                              <path d="M1 2h2M5 2h2M1 7h2M5 7h2M1 12h2M5 12h2" stroke="currentColor" stroke-width="2" />
                            </svg>
                            {i() + 1}
                          </span>
                          <button type="button" class="patch-fx-body" onClick={() => (select(f.id), setUniverse(f.universe))}>
                            <b>
                              {f.name}
                              <Show when={store.inConflict(f.id)}>
                                <i class="patch-clash-mark" aria-label="address clash" />
                              </Show>
                            </b>
                            <span class="mute">
                              {f.model} · {f.mode}
                            </span>
                            <small>{range(f)}</small>
                          </button>
                          <span class="patch-move">
                            <button type="button" aria-label={`Move ${f.name} earlier`} disabled={i() === 0} onClick={() => store.reorderFixture(f.id, -1)}>
                              <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
                                <path d="M2 8l4-4 4 4" fill="none" stroke="currentColor" stroke-width="2.5" />
                              </svg>
                            </button>
                            <button type="button" aria-label={`Move ${f.name} later`} disabled={i() === store.rig.filter((x) => x.group === g.id).length - 1} onClick={() => store.reorderFixture(f.id, 1)}>
                              <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
                                <path d="M2 4l4 4 4-4" fill="none" stroke="currentColor" stroke-width="2.5" />
                              </svg>
                            </button>
                          </span>
                        </li>
                      )}
                    </For>
                  </ol>
                </section>
              )}
            </For>
          </div>
        </Region>
      </div>

      <Region id="details" title={flow.open ? "Add lights" : side() === "panel" ? "Add a pixel panel" : "Details"} bare class="patch-side">
        <Show when={!flow.open} fallback={<AddFlow onDone={(f) => (setSelectedId(f.id), setUniverse(1))} />}>
          <Show
            when={side() === "panel"}
            fallback={
              <Show
                when={panel()}
                fallback={
                  <Show when={selected()} fallback={<p class="mute">Pick a light in the grid or the list.</p>}>
                    {(f) => (
                      <div class="patch-details">
                        <header>
                          <h2 class="display">{f().name}</h2>
                          <span class="mute">
                            {f().model} · {f().mode}
                          </span>
                        </header>
                        <dl>
                          <div>
                            <dt>Group</dt>
                            <dd>{store.groups.find((g) => g.id === f().group)?.name}</dd>
                          </div>
                          <div>
                            <dt>Channels</dt>
                            <dd>{f().channels}</dd>
                          </div>
                          <div>
                            <dt>Universe</dt>
                            <dd>{f().universe}</dd>
                          </div>
                        </dl>
                        <div class="patch-address">
                          <span class="mute">Address</span>
                          <div class="patch-stepper">
                            <Button aria-label="Address down" onClick={() => store.setAddress(f().id, f().address - 1)}>
                              <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
                                <path d="M2 7h10" stroke="currentColor" stroke-width="3" />
                              </svg>
                            </Button>
                            <span class="patch-addr-big">
                              <b class="display">{f().address}</b>
                              <span class="mute">to {f().address + f().channels - 1}</span>
                            </span>
                            <Button aria-label="Address up" onClick={() => store.setAddress(f().id, f().address + 1)}>
                              <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
                                <path d="M2 7h10M7 2v10" stroke="currentColor" stroke-width="3" />
                              </svg>
                            </Button>
                          </div>
                        </div>
                        <Show when={other(f().id)}>
                          {(o) => (
                            <div class="patch-clash-box" role="alert">
                              <p>
                                <strong>Clashes with {o().name}</strong>. Both lights get the same data there.
                              </p>
                              <Button
                                onClick={() => {
                                  const a = store.freeAddress(f().channels, f().universe, f().id);
                                  if (a) store.setAddress(f().id, a);
                                }}
                              >
                                Move to {store.freeAddress(f().channels, f().universe, f().id) ?? "no free"}
                              </Button>
                            </div>
                          )}
                        </Show>
                        <Button pressed={store.state.identify === f().id} onClick={() => store.setIdentify(store.state.identify === f().id ? null : f().id)}>
                          {store.state.identify === f().id ? "Stop flashing" : "Flash it, to find it in the room"}
                        </Button>
                      </div>
                    )}
                  </Show>
                }
              >
                {(p) => (
                  <div class="patch-details">
                    <header>
                      <h2 class="display">{p().name}</h2>
                      <span class="mute">
                        {p().cols} x {p().rows} {p().pixel} pixels, order {p().pixel === "RGBW" ? `${p().order}W` : p().order}, {p().wiring === "snake" ? "snake" : "zigzag"} wiring
                      </span>
                    </header>
                    <ul class="patch-panel-runs">
                      <For each={store.panelRuns(p())}>
                        {(r) => (
                          <li>
                            <button type="button" class="patch-uni" aria-current={universe() === r.universe ? "true" : undefined} onClick={() => setUniverse(r.universe)}>
                              <b>Universe {r.universe}</b>
                              <span class="mute">
                                pixels {r.firstPixel} to {r.lastPixel}, {r.channels} channels
                              </span>
                            </button>
                          </li>
                        )}
                      </For>
                    </ul>
                  </div>
                )}
              </Show>
            }
          >
            <form
              class="patch-panel-form"
              onSubmit={(e) => {
                e.preventDefault();
                const start = ppStart();
                const id = store.addPanel({ ...pp(), startUniverse: start });
                setPanelId(id);
                setUniverse(start);
                setSide("details");
              }}
            >
              <h2 class="display">Add a pixel panel</h2>
              <label>
                <span class="mute">Name</span>
                <input type="text" value={pp().name} onInput={(e) => setPp({ ...pp(), name: e.currentTarget.value })} />
              </label>
              <div class="patch-two">
                <label>
                  <span class="mute">Columns</span>
                  <input type="text" inputMode="numeric" value={pp().cols} onInput={(e) => setPp({ ...pp(), cols: Math.max(1, Number(e.currentTarget.value) || 1) })} />
                </label>
                <label>
                  <span class="mute">Rows</span>
                  <input type="text" inputMode="numeric" value={pp().rows} onInput={(e) => setPp({ ...pp(), rows: Math.max(1, Number(e.currentTarget.value) || 1) })} />
                </label>
              </div>
              <span class="mute">Wiring</span>
              <Segmented
                label="Wiring"
                value={pp().wiring}
                onChange={(v) => setPp({ ...pp(), wiring: v })}
                options={[
                  { value: "snake", label: "Snake" },
                  { value: "zigzag", label: "Zigzag" },
                ]}
              />
              <svg class="patch-wiring" viewBox="0 0 120 40" aria-hidden="true">
                <path d={pp().wiring === "snake" ? "M6 8H114V20H6V32H114" : "M6 8H114L6 20H114L6 32H114"} />
              </svg>
              <div class="patch-two">
                <div class="patch-field">
                  <span class="mute">Pixel type</span>
                  <Segmented
                    label="Pixel type"
                    value={pp().pixel}
                    onChange={(v) => setPp({ ...pp(), pixel: v })}
                    options={[
                      { value: "RGB", label: "RGB" },
                      { value: "RGBW", label: "RGBW" },
                    ]}
                  />
                </div>
                <label>
                  <span class="mute">Colour order</span>
                  <select value={pp().order} onChange={(e) => setPp({ ...pp(), order: e.currentTarget.value as ColourOrder })}>
                    <For each={COLOUR_ORDERS}>{(o) => <option value={o}>{pp().pixel === "RGBW" ? `${o}W` : o}</option>}</For>
                  </select>
                </label>
              </div>
              <label>
                <span class="mute">Start universe</span>
                <input type="text" inputMode="numeric" value={ppStart()} onInput={(e) => setPp({ ...pp(), startUniverse: Math.max(1, Number(e.currentTarget.value) || 1) })} />
              </label>
              <p class="patch-panel-sum">
                {pp().cols * pp().rows} {pp().pixel} pixels, {ppPerUniverse()} per universe, need {ppUniverses()} {ppUniverses() === 1 ? "universe" : "universes"}: {ppStart()}
                {ppUniverses() > 1 ? ` to ${ppStart() + ppUniverses() - 1}` : ""}.
              </p>
              <Button type="submit" variant="primary">
                Add the panel
              </Button>
            </form>
          </Show>
        </Show>
      </Region>
    </div>
  );
}
