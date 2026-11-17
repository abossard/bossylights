// The console: pages of widgets on a 12 x 12 grid, and edit mode to arrange them.
import { createSignal, For, Show } from "solid-js";
import { createStore, produce } from "solid-js/store";
import { store } from "../../fake/store";
import { Button } from "../../kit";
import { COLS, DRAWER, KINDS, ROWS, WidgetBody, type Widget, type WidgetKind } from "./widgets";

export interface Page {
  id: string;
  name: string;
  widgets: Widget[];
}

const initialPages = (): Page[] => [
  {
    id: "main",
    name: "Main",
    widgets: [
      { id: "pads", kind: "pads", title: "Pads", x: 0, y: 0, w: 12, h: 7, ref: "main" },
      { id: "faders", kind: "faders", title: "Faders", x: 0, y: 7, w: 7, h: 5, ref: "main" },
      { id: "xy", kind: "xy", title: "Movers position", x: 7, y: 7, w: 5, h: 5 },
    ],
  },
  {
    id: "movers",
    name: "Movers",
    widgets: [
      { id: "pads", kind: "pads", title: "Pads", x: 0, y: 0, w: 12, h: 4, ref: "movers" },
      { id: "xy", kind: "xy", title: "Movers position", x: 0, y: 4, w: 8, h: 8 },
      { id: "faders", kind: "faders", title: "Faders", x: 8, y: 4, w: 4, h: 8, ref: "movers" },
    ],
  },
  {
    id: "pixels",
    name: "Pixels",
    widgets: [
      { id: "pads", kind: "pads", title: "Pads", x: 0, y: 0, w: 12, h: 4, ref: "pixels" },
      { id: "faders", kind: "faders", title: "Faders", x: 0, y: 4, w: 8, h: 8, ref: "pixels" },
      { id: "speed", kind: "speed", title: "Speed", x: 8, y: 4, w: 4, h: 3 },
    ],
  },
];

type Rect = Pick<Widget, "x" | "y" | "w" | "h">;

const overlaps = (a: Rect, b: Rect) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
const inside = (r: Rect) => r.x >= 0 && r.y >= 0 && r.x + r.w <= COLS && r.y + r.h <= ROWS;

/** Pages, the current page and edit mode. Shared by the page bar and the grid. */
export function createConsole() {
  const [pages, setPages] = createStore<Page[]>(initialPages());
  const [pageId, setPageId] = createSignal("main");
  const [editing, setEditing] = createSignal(false);
  const [message, setMessage] = createSignal("");
  let seq = 1;

  const page = () => pages.find((p) => p.id === pageId()) ?? pages[0];
  const pageIndex = () => pages.findIndex((p) => p.id === page().id);

  /** Frames never block: other widgets may sit on them. */
  const fits = (r: Rect, kind: WidgetKind, except?: string) =>
    inside(r) &&
    (kind === "frame" || !page().widgets.some((w) => w.id !== except && w.kind !== "frame" && overlaps(r, w)));

  function place(id: string, r: Rect) {
    setPages(pageIndex(), "widgets", (w) => w.id === id, r);
  }

  function addPage() {
    const n = pages.length + 1;
    const id = `page-${n}-${seq++}`;
    setPages((ps) => [...ps, { id, name: `Page ${n}`, widgets: [] }]);
    const before = pageId();
    setPageId(id);
    store.record("Add page", () => {
      setPages((ps) => ps.filter((p) => p.id !== id));
      setPageId(before);
    });
  }

  function addWidget(kind: WidgetKind) {
    const k = KINDS[kind];
    const tryAt = (w: number, h: number) => {
      for (let y = 0; y + h <= ROWS; y++)
        for (let x = 0; x + w <= COLS; x++) if (fits({ x, y, w, h }, kind)) return { x, y, w, h };
      return null;
    };
    const spot = tryAt(k.w, k.h) ?? tryAt(k.minW, k.minH);
    if (!spot) {
      setMessage(`No free space for a ${k.name.toLowerCase()} on this page. Make a widget smaller first.`);
      return;
    }
    const id = `${kind}-${seq++}`;
    const widget: Widget = {
      id,
      kind,
      title: k.name,
      ...spot,
      ref: kind === "button" ? "blinder" : kind === "slider" ? "strobeRate" : undefined,
    };
    const pi = pageIndex();
    setPages(pi, "widgets", (ws) => [...ws, widget]);
    setMessage(`Added a ${k.name.toLowerCase()}. Drag it to move it, drag its corner to resize.`);
    store.record(`Add ${k.name}`, () => setPages(pi, "widgets", (ws) => ws.filter((w) => w.id !== id)));
  }

  function removeWidget(id: string) {
    const pi = pageIndex();
    const w = pages[pi].widgets.find((x) => x.id === id);
    if (!w) return;
    const copy = { ...w };
    setPages(pi, "widgets", (ws) => ws.filter((x) => x.id !== id));
    store.record(`Remove ${KINDS[w.kind].name}`, () => setPages(pi, "widgets", (ws) => [...ws, copy]));
  }

  /** Called once a drag or resize ends, so it is one undo step. */
  function recordPlace(id: string, before: Rect, label: string) {
    const pi = pageIndex();
    store.record(label, () => setPages(pi, "widgets", (w) => w.id === id, produce((w: Widget) => Object.assign(w, before))));
  }

  return {
    pages,
    page,
    pageId,
    setPageId,
    editing,
    setEditing,
    message,
    setMessage,
    fits,
    place,
    addPage,
    addWidget,
    removeWidget,
    recordPlace,
  };
}

export type ConsoleModel = ReturnType<typeof createConsole>;

/** The grid of widgets. In edit mode: the grid shows, widgets move and resize. */
export function ConsoleGrid(props: { model: ConsoleModel }) {
  const m = props.model;
  let grid!: HTMLDivElement;

  const cell = () => {
    const r = grid.getBoundingClientRect();
    const gap = parseFloat(getComputedStyle(grid).columnGap) || 0;
    return { w: (r.width + gap) / COLS, h: (r.height + gap) / ROWS };
  };

  /** Pointer drag that moves (or resizes) a widget by whole cells. */
  const drag = (w: Widget, mode: "move" | "resize") => (e: PointerEvent) => {
    if (e.button > 0) return;
    e.stopPropagation();
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const start = { x: e.clientX, y: e.clientY };
    const before: Rect = { x: w.x, y: w.y, w: w.w, h: w.h };
    const c = cell();
    const k = KINDS[w.kind];
    const move = (ev: PointerEvent) => {
      const dx = Math.round((ev.clientX - start.x) / c.w);
      const dy = Math.round((ev.clientY - start.y) / c.h);
      const next =
        mode === "move"
          ? { ...before, x: before.x + dx, y: before.y + dy }
          : { ...before, w: Math.max(k.minW, before.w + dx), h: Math.max(k.minH, before.h + dy) };
      if (m.fits(next, w.kind, w.id)) m.place(w.id, next);
    };
    const end = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", end);
      el.removeEventListener("pointercancel", end);
      const now = { x: w.x, y: w.y, w: w.w, h: w.h };
      if (JSON.stringify(now) !== JSON.stringify(before)) m.recordPlace(w.id, before, mode === "move" ? "Move widget" : "Resize widget");
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", end);
    el.addEventListener("pointercancel", end);
  };

  /** Arrow keys move a widget, Shift+arrows resize it. */
  const key = (w: Widget) => (e: KeyboardEvent) => {
    const d: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      m.removeWidget(w.id);
      return;
    }
    if (!d[e.key]) return;
    e.preventDefault();
    const [dx, dy] = d[e.key];
    const before: Rect = { x: w.x, y: w.y, w: w.w, h: w.h };
    const k = KINDS[w.kind];
    const next = e.shiftKey
      ? { ...before, w: Math.max(k.minW, w.w + dx), h: Math.max(k.minH, w.h + dy) }
      : { ...before, x: w.x + dx, y: w.y + dy };
    if (m.fits(next, w.kind, w.id)) {
      m.place(w.id, next);
      m.recordPlace(w.id, before, e.shiftKey ? "Resize widget" : "Move widget");
    }
  };

  return (
    <div ref={grid} class="console-grid" classList={{ "is-editing": m.editing() }}>
      <Show when={m.editing()}>
        <For each={Array.from({ length: COLS * ROWS }, (_, i) => i)}>
          {(i) => (
            <i class="console-cell" aria-hidden="true" style={{ "grid-column": (i % COLS) + 1, "grid-row": Math.floor(i / COLS) + 1 }} />
          )}
        </For>
      </Show>
      <For each={m.page().widgets}>
        {(w) => (
          <div
            class={`widget widget-${w.kind}`}
            style={{ "grid-column": `${w.x + 1} / span ${w.w}`, "grid-row": `${w.y + 1} / span ${w.h}` }}
          >
            <WidgetBody widget={w} editing={m.editing()} pageId={m.page().id} />
            <Show when={m.editing()}>
              <div
                class="w-edit"
                tabIndex={0}
                role="group"
                aria-label={`${w.title}, ${w.w} by ${w.h}. Arrow keys move, Shift and arrows resize, Delete removes.`}
                onPointerDown={drag(w, "move")}
                onKeyDown={key(w)}
              >
                <span class="w-edit-name">
                  {w.title} <span>{w.w} x {w.h}</span>
                </span>
                <button
                  type="button"
                  class="w-edit-remove"
                  aria-label={`Remove ${w.title}`}
                  onPointerDown={(e) => e.stopPropagation()}
                  onClick={() => m.removeWidget(w.id)}
                >
                  <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
                    <path d="M3 3l10 10M13 3L3 13" stroke="currentColor" stroke-width="3" />
                  </svg>
                </button>
                <span class="w-edit-resize" onPointerDown={drag(w, "resize")} aria-hidden="true">
                  <svg width="16" height="16" viewBox="0 0 16 16">
                    <path d="M15 5v10H5" fill="none" stroke="currentColor" stroke-width="3" />
                  </svg>
                </span>
              </div>
            </Show>
          </div>
        )}
      </For>
      <Show when={!m.page().widgets.length}>
        <div class="console-empty">
          <p class="display">{m.page().name} is empty</p>
          <p class="mute">Add widgets from the drawer. Drag them into place, drag a corner to resize.</p>
          <Show when={!m.editing()}>
            <Button variant="primary" disabled={store.surfaceLocked()} reason="Locked while LIVE is on" onClick={() => m.setEditing(true)}>
              Edit surface
            </Button>
          </Show>
        </div>
      </Show>
    </div>
  );
}

/** The widget drawer, open while editing. Tap a widget to add it to the page. */
export function WidgetDrawer(props: { model: ConsoleModel }) {
  return (
    <aside class="drawer" aria-label="Widget drawer">
      <header class="drawer-head">
        <h2 class="display">Add a widget</h2>
        <p class="mute">Tap one to put it on {props.model.page().name}. Drag to move, drag the corner to resize.</p>
      </header>
      <ul class="drawer-list">
        <For each={DRAWER}>
          {(kind) => (
            <li>
              <button type="button" class="drawer-item" onClick={() => props.model.addWidget(kind)}>
                <b class="display">{KINDS[kind].name}</b>
                <span class="mute">{KINDS[kind].about}</span>
              </button>
            </li>
          )}
        </For>
      </ul>
      <p class="drawer-msg mute" role="status">
        {props.model.message()}
      </p>
    </aside>
  );
}
