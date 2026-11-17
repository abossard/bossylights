import { createMemo, createSignal, For, onCleanup, Show } from "solid-js";
import { Button, Region } from "../../kit";
import { QUESTIONS } from "../../review/questions";
import { review } from "../../review/store";
import type { Issue } from "../../shell/registry";
import { go } from "../../shell/router";
import { ARROWS, MODULES, TRACE, WRITERS, type Module } from "./data";
import "./modules.css";

export const id = "modules";
export const title = "Modules";
export const order = 8;
export const issues: Issue[] = [
  { number: 100, title: "Review the theory and the module boundary map" },
  { number: 26, title: "One engine, one API, no UI inside" },
];

const W = 130;
const H = 52;
const HOP_MS = 700;
const reduced = matchMedia("(prefers-reduced-motion: reduce)");

export function Screen() {
  const [selected, setSelected] = createSignal("mixer");
  const mod = () => MODULES.find((m) => m.id === selected())!;
  const paths: Record<string, SVGPathElement> = {};

  // ---- trace a press: a dot hops Surface, Engine, Mixer, Rig, Output and back
  const [trace, setTrace] = createSignal<{ t0: number } | null>(null);
  const [tick, setTick] = createSignal(0);
  let raf = 0;
  const loop = () => {
    const t = trace();
    if (!t) return;
    const now = performance.now();
    setTick(now);
    if (now - t.t0 > TRACE.length * HOP_MS + 1200) {
      setTrace(null);
      return;
    }
    raf = requestAnimationFrame(loop);
  };
  const start = () => {
    cancelAnimationFrame(raf);
    setTrace({ t0: performance.now() });
    raf = requestAnimationFrame(loop);
  };
  onCleanup(() => cancelAnimationFrame(raf));
  const hop = () => {
    const t = trace();
    tick();
    if (!t) return -1;
    return Math.min(TRACE.length - 1, Math.floor((performance.now() - t.t0) / HOP_MS));
  };
  const dot = () => {
    const t = trace();
    const i = hop();
    if (!t || i < 0) return null;
    const h = TRACE[i];
    const el = paths[h.path];
    if (!el) return null;
    let f = reduced.matches ? 1 : Math.min(1, (performance.now() - t.t0 - i * HOP_MS) / (HOP_MS * 0.7));
    if (h.reverse) f = 1 - f;
    const p = el.getPointAtLength(el.getTotalLength() * f);
    return { x: p.x, y: p.y };
  };
  const tracing = (id: string) => trace() !== null && TRACE[hop()]?.at === id;
  const traced = (pathId: string) => trace() !== null && TRACE.slice(0, hop() + 1).some((h) => h.path === pathId);

  // ---- questions
  const answered = createMemo(() => QUESTIONS.filter((q) => q.decided || review.answerOf(q.id).choice || review.answerOf(q.id).text.trim()).length);
  // a screen must not import the registry (it would import itself), so titles are local
  const SCREEN_TITLES: Record<string, string> = {
    console: "Console", songs: "Songs", looks: "Looks", patch: "Patch", dmx: "DMX", inout: "In/Out",
  };
  const screenTitle = (id: string) => SCREEN_TITLES[id] ?? id;

  const Node = (m: Module) => (
    <g
      class="mm-node"
      classList={{ "is-core": m.core, "is-selected": selected() === m.id, "is-traced": tracing(m.id), "is-engine": m.id === "engine" }}
      role="button"
      tabIndex={0}
      aria-pressed={selected() === m.id}
      aria-label={`${m.name}${m.core ? ", real-time core" : ""}`}
      onClick={() => setSelected(m.id)}
      onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), setSelected(m.id))}
    >
      <rect x={m.x} y={m.y} width={m.w ?? W} height={m.h ?? H} />
      <text x={m.x + (m.w ?? W) / 2} y={m.y + (m.h ?? H) / 2 + (m.sub ? -3 : 7)} text-anchor="middle">
        {m.name}
      </text>
      <Show when={m.sub}>
        <text class="mm-sub" x={m.x + (m.w ?? W) / 2} y={m.y + (m.h ?? H) / 2 + 17} text-anchor="middle">
          {m.sub}
        </text>
      </Show>
    </g>
  );

  return (
    <div class="mm-screen">
      <Region
        id="map"
        title="The boundary map"
        class="mm-map"
        aside={
          <Button variant="primary" pressed={trace() !== null} onClick={start}>
            Trace a press
          </Button>
        }
      >
        <svg class="mm-svg" viewBox="0 0 940 446" role="img" aria-label="The twelve modules. Every writer goes through the Engine. The core never calls outward.">
          <defs>
            <marker id="mm-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
              <path d="M0,0 L10,5 L0,10 z" class="mm-head" />
            </marker>
          </defs>
          <rect class="mm-core" x="525" y="128" width="410" height="290" />
          <text class="mm-core-label" x="935" y="120" text-anchor="end">
            Real-time core, never waits
          </text>
          <For each={ARROWS}>
            {(a) => (
              <path
                ref={(el) => (paths[a.id] = el)}
                d={a.d}
                class="mm-arrow"
                classList={{ "is-state": !!a.state, "is-traced": traced(a.id) }}
                marker-end="url(#mm-arrow)"
              />
            )}
          </For>
          <For each={WRITERS}>
            {(w) => (
              <g class="mm-writer">
                <rect x={w.x} y={w.y} width={W} height={48} />
                <text x={w.x + W / 2} y={w.y + 31} text-anchor="middle">
                  {w.name}
                </text>
              </g>
            )}
          </For>
          <For each={MODULES}>{Node}</For>
          <Show when={dot()}>{(p) => <circle class="mm-dot" cx={p().x} cy={p().y} r="10" />}</Show>
        </svg>
        <p class="mm-caption" role="status">
          <Show
            when={trace() !== null}
            fallback={<span class="mute">Solid arrows: commands, one way only. Dotted: state coming back. Tap a module to open it.</span>}
          >
            <b>{hop() + 1}.</b> <code>{TRACE[hop()].say}</code>
          </Show>
        </p>
      </Region>

      <Region id="card" title="Module" bare class="mm-card">
        <header class="mm-card-head">
          <h2 class="display">{mod().name}</h2>
          <Show when={mod().sub}>
            <span class="mm-card-sub">{mod().sub}</span>
          </Show>
          <Show when={mod().core}>
            <span class="mm-core-chip">Real-time core</span>
          </Show>
        </header>
        <section>
          <h3 class="mm-h">Whole interface</h3>
          <p class="mm-iface">
            <For each={mod().iface}>{(f) => <code>{f}</code>}</For>
          </p>
        </section>
        <section>
          <h3 class="mm-h">Hides</h3>
          <ul class="mm-hides">
            <For each={mod().hides}>{(h) => <li>{h}</li>}</For>
          </ul>
        </section>
        <section aria-label={`Depth: interface ${mod().ifaceSize}, hides ${mod().hidesSize}`}>
          <h3 class="mm-h">Depth</h3>
          <div class="mm-depth">
            <span class="mm-depth-label">Interface</span>
            <span class="mm-bar is-iface" style={{ width: `${mod().ifaceSize * 10}%` }} />
            <span class="mm-depth-label">Hides</span>
            <span class="mm-bar is-hides" style={{ width: `${mod().hidesSize * 10}%` }} />
          </div>
          <p class="mute">
            {mod().hidesSize / mod().ifaceSize >= 3 ? "Deep: a small interface hides a lot." : "Wider than the others: watch that it stays narrow."}
          </p>
        </section>
        <section>
          <h3 class="mm-h">Used by</h3>
          <p class="mm-screens">
            <Show when={mod().screens.length} fallback={<span class="mute">No screen directly: they reach it through the Mixer.</span>}>
              <For each={mod().screens}>
                {(s) => (
                  <Button class="mm-screen-link" onClick={() => go(s)}>
                    {screenTitle(s)}
                  </Button>
                )}
              </For>
            </Show>
          </p>
        </section>
      </Region>

      <Region id="questions" title="Questions for you" class="mm-questions" aside={<span class="mute">{answered()} of {QUESTIONS.length} answered · saved with your review</span>}>
        <div class="mm-q-cols">
          <For each={["Theory", "Modules"] as const}>
            {(src) => (
              <section>
                <h3 class="mm-h">From the {src === "Theory" ? "theory" : "module map"}</h3>
                <ol class="mm-q-list">
                  <For each={QUESTIONS.filter((q) => q.source === src)}>
                    {(q) => {
                      const a = () => review.answerOf(q.id);
                      return (
                        <li class="mm-q">
                          <p class="mm-q-text">
                            <b>{q.number}.</b> {q.question}
                          </p>
                          <Show when={q.decided}>
                            <p class="mm-q-decided">
                              <b>Decided</b> {q.decided}
                            </p>
                          </Show>
                          <Show when={q.a && !q.decided}>
                            <div class="mm-q-opts" role="radiogroup" aria-label={q.question}>
                              <For each={[{ k: "A" as const, t: q.a }, { k: "B" as const, t: q.b }]}>
                                {(o) => (
                                  <button
                                    type="button"
                                    role="radio"
                                    class="mm-q-opt"
                                    aria-checked={a().choice === o.k}
                                    onClick={() => review.setChoice(q.id, a().choice === o.k ? null : o.k)}
                                  >
                                    <b>{o.k}</b>
                                    <span>{o.t}</span>
                                    <Show when={o.k === "A"}>
                                      <em>the author's lean</em>
                                    </Show>
                                  </button>
                                )}
                              </For>
                            </div>
                          </Show>
                          <input
                            type="text"
                            aria-label={`Your words on: ${q.question}`}
                            placeholder={q.a ? "Your words (optional)" : "Your answer"}
                            value={a().text}
                            onInput={(e) => review.setAnswerText(q.id, e.currentTarget.value)}
                          />
                        </li>
                      );
                    }}
                  </For>
                </ol>
              </section>
            )}
          </For>
        </div>
      </Region>
    </div>
  );
}
