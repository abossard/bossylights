// The beat grid: one row per item, one beat per item by default.
// Drag an item to move it, drag its right edge to change its width.
import { For, Show } from "solid-js";
import { store, type Entry, type Sequence, type Speed } from "../../fake/store";
import { Button, Fader, Segmented } from "../../kit";
import { KindSymbol } from "./Symbol";
import { SequenceItems } from "./Manage";

const SNAP = 0.25;
const snap = (v: number) => Math.round(v / SNAP) * SNAP;
const beats = (b: number) => (b === 1 ? "1 beat" : `${+b.toFixed(2)} beats`);
const SPEED_LABEL: Record<Speed, string> = { 0.5: "÷2", 1: "×1", 2: "×2" };

export function SequenceEditor(props: {
  seq: Sequence;
  selected: string | null;
  onSelect: (entryId: string) => void;
  onOpen: (ref: string) => void;
}) {
  let track!: HTMLDivElement;
  const measure = () => store.measureOf(props.seq.id) ?? 0;
  /** Columns on the grid: the measure plus room to grow. */
  const cols = () => Math.max(4, Math.ceil(measure())) + 2;
  const pct = (b: number) => (b / cols()) * 100;
  const entry = () => props.seq.entries.find((x) => x.id === props.selected) ?? null;

  const drag = (en: Entry, how: "move" | "width") => (e: PointerEvent) => {
    if (e.button > 0) return;
    e.stopPropagation();
    props.onSelect(en.id);
    const el = e.currentTarget as HTMLElement;
    try {
      el.setPointerCapture(e.pointerId);
    } catch {
      // synthetic pointer: drag still works
    }
    const x0 = e.clientX;
    const beatPx = track.getBoundingClientRect().width / cols();
    const s0 = en.start;
    const w0 = en.width;
    const move = (ev: PointerEvent) => {
      const d = (ev.clientX - x0) / beatPx;
      if (how === "move") store.setEntry(props.seq.id, en.id, { start: Math.max(0, snap(s0 + d)) }, "Move item");
      else store.setEntry(props.seq.id, en.id, { width: Math.max(SNAP, snap(w0 + d)) }, "Change width");
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

  /** Arrows move by a quarter beat, Shift and arrows change the width. */
  const key = (en: Entry) => (e: KeyboardEvent) => {
    const d = e.key === "ArrowLeft" ? -SNAP : e.key === "ArrowRight" ? SNAP : 0;
    if (!d) return;
    e.preventDefault();
    if (e.shiftKey) store.setEntry(props.seq.id, en.id, { width: Math.max(SNAP, en.width + d) }, "Change width");
    else store.setEntry(props.seq.id, en.id, { start: Math.max(0, en.start + d) }, "Move item");
  };

  const moverWarning = (en: Entry) => en.speed === 2 && store.fixturesOf(en.ref).includes("movers");

  return (
    <div class="lk-seq">
      <div class="lk-grid" style={{ "--cols": cols() }}>
        <div class="lk-grid-row lk-ruler">
          <span class="lk-row-label mute">Item</span>
          <div class="lk-track" aria-hidden="true">
            <For each={Array.from({ length: cols() }, (_, i) => i)}>
              {(i) => (
                <span class="lk-beat" style={{ left: `${pct(i)}%`, width: `${pct(1)}%` }}>
                  {i + 1}
                </span>
              )}
            </For>
          </div>
        </div>
        <div class="lk-rows">
          <For each={props.seq.entries}>
            {(en) => {
              const it = () => store.item(en.ref);
              return (
                <div class="lk-grid-row" classList={{ "is-selected": props.selected === en.id }}>
                  <div class="lk-row-label">
                    <button type="button" class="lk-row-name" aria-pressed={props.selected === en.id} onClick={() => props.onSelect(en.id)}>
                      <KindSymbol kind={it()?.kind ?? "step"} />
                      <span>{it()?.name ?? en.ref}</span>
                    </button>
                    <Button class="lk-open" aria-label={`Open ${it()?.name}`} onClick={() => props.onOpen(en.ref)}>
                      <span class="lk-open-word">Open</span>
                      <svg width="10" height="12" viewBox="0 0 10 12" aria-hidden="true">
                        <path d="M2 1l6 5-6 5" fill="none" stroke="currentColor" stroke-width="2.5" />
                      </svg>
                    </Button>
                  </div>
                  <div ref={(el) => (track = el)} class="lk-track">
                    <For each={Array.from({ length: cols() }, (_, i) => i)}>{(i) => <i class="lk-line" style={{ left: `${pct(i)}%` }} />}</For>
                    <div
                      class="lk-bar"
                      classList={{ "is-selected": props.selected === en.id }}
                      style={{ left: `${pct(en.start)}%`, width: `${pct(en.width)}%` }}
                      role="slider"
                      tabIndex={0}
                      aria-label={`${it()?.name}: starts at beat ${en.start + 1}, ${beats(en.width)}. Arrows move it, Shift and arrows change the width.`}
                      aria-valuenow={en.width}
                      onPointerDown={drag(en, "move")}
                      onKeyDown={key(en)}
                    >
                      <span class="lk-bar-chips">
                        <For each={store.colorsOf(en.ref).slice(0, 4)}>{(c) => <i style={{ background: c }} />}</For>
                      </span>
                      <span class="lk-bar-text">
                        {beats(en.width)}
                        <Show when={en.speed !== 1}> · {SPEED_LABEL[en.speed]}</Show>
                        <Show when={!en.loop}> · once</Show>
                      </span>
                      <span class="lk-handle" onPointerDown={drag(en, "width")} aria-hidden="true" />
                    </div>
                  </div>
                </div>
              );
            }}
          </For>
          <span class="lk-measure" style={{ left: `calc(var(--label-w) + (100% - var(--label-w)) * ${measure() / cols()})` }} aria-hidden="true">
            <b>Loop</b>
          </span>
        </div>
      </div>
      <p class="lk-measure-text">
        Measure <b>{beats(measure())}</b>, then it loops. Drag an item to move it, drag its right edge to change its width.
      </p>

      <SequenceItems seqName={props.seq.name} selected={entry() ? store.item(entry()!.ref)?.name : undefined} />
      <Show when={entry()} fallback={<p class="mute">Pick an item to see its use settings.</p>}>
        {(en) => (
          <section class="lk-use" aria-label="Use settings">
            <h3>
              {store.item(en().ref)?.name} <span class="mute">in {props.seq.name}</span>
            </h3>
            <div class="lk-use-grid">
              <div class="lk-use-field">
                <span class="lk-label">Width</span>
                <div class="lk-stepper">
                  <Button aria-label="Narrower" disabled={en().width <= SNAP} onClick={() => store.setEntry(props.seq.id, en().id, { width: Math.max(SNAP, en().width - SNAP) }, "Change width")}>
                    −
                  </Button>
                  <b class="display">{beats(en().width)}</b>
                  <Button aria-label="Wider" onClick={() => store.setEntry(props.seq.id, en().id, { width: en().width + SNAP }, "Change width")}>
                    +
                  </Button>
                </div>
              </div>
              <div class="lk-use-field">
                <span class="lk-label">Speed</span>
                <Segmented
                  label="Speed"
                  value={String(en().speed)}
                  onChange={(v) => store.setEntry(props.seq.id, en().id, { speed: Number(v) as Speed }, "Change speed")}
                  options={[
                    { value: "0.5", label: "÷2" },
                    { value: "1", label: "×1" },
                    { value: "2", label: "×2" },
                  ]}
                />
              </div>
              <div class="lk-use-field">
                <span class="lk-label">Repeat</span>
                <Segmented
                  label="Loop or once"
                  value={en().loop ? "loop" : "once"}
                  onChange={(v) => store.setEntry(props.seq.id, en().id, { loop: v === "loop" }, "Loop or once")}
                  options={[
                    { value: "loop", label: "Loop" },
                    { value: "once", label: "Once" },
                  ]}
                />
              </div>
              <div class="lk-use-field">
                <span class="lk-label">
                  Spread <span class="mute">{en().spread === 0 ? "all together" : `${en().spread}% across the fixtures`}</span>
                </span>
                <Fader orientation="horizontal" bare label="Spread across fixtures" value={en().spread} onChange={(v) => store.setEntry(props.seq.id, en().id, { spread: v }, "Spread")} />
              </div>
              <div class="lk-use-field">
                <span class="lk-label">
                  Transition <span class="mute">{en().transition === 0 ? "a cut" : `${en().transition}% move, the rest holds`}</span>
                </span>
                <Fader orientation="horizontal" bare label="Transition" value={en().transition} onChange={(v) => store.setEntry(props.seq.id, en().id, { transition: v }, "Transition")} />
              </div>
            </div>
            <Show when={moverWarning(en())}>
              <p class="lk-warn" role="status">
                At ×2 the movers may not reach every position. They will cut the corners.
              </p>
            </Show>
          </section>
        )}
      </Show>
    </div>
  );
}
