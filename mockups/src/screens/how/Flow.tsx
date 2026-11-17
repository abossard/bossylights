// The whole theory in one picture: writers, layers, frame, rig, output.
// Commands travel along it as dots.
import { createSignal, For, onCleanup, Show } from "solid-js";
import { CLOCK_START, lookById, store } from "../../fake/store";

export type NodeId = "you" | "controller" | "agent" | "showw" | "autow" | "manual" | "show" | "auto" | "frame" | "rig" | "output";

interface Box {
  id: NodeId;
  x: number;
  y: number;
  w: number;
  h: number;
  title: string;
  sub?: string;
  layer?: "manual" | "show" | "autopilot";
}

const WRITERS: Box[] = [
  { id: "you", x: 16, y: 16, w: 196, h: 54, title: "You", sub: "screen, touch" },
  { id: "controller", x: 16, y: 80, w: 196, h: 54, title: "Controller", sub: "Launchpad, OSC" },
  { id: "agent", x: 16, y: 144, w: 196, h: 54, title: "AI agent", sub: "MCP" },
  { id: "showw", x: 16, y: 208, w: 196, h: 54, title: "Show", sub: "replays commands" },
  { id: "autow", x: 16, y: 272, w: 196, h: 54, title: "Autopilot", sub: "commands from music" },
];
const LAYERS: Box[] = [
  { id: "manual", x: 400, y: 26, w: 250, h: 84, title: "Manual", sub: "your hands", layer: "manual" },
  { id: "show", x: 400, y: 128, w: 250, h: 84, title: "Show", layer: "show" },
  { id: "auto", x: 400, y: 230, w: 250, h: 84, title: "Autopilot", layer: "autopilot" },
];
const RIG: Box = { id: "rig", x: 900, y: 140, w: 130, h: 60, title: "Rig", sub: "lights to DMX" };
const OUT: Box = { id: "output", x: 1058, y: 140, w: 126, h: 60, title: "Output", sub: "USB, Art-Net" };

/** Paths a command travels, by the node it leads to. */
export const PATHS: Record<string, string> = {
  "p-you": "M212,43 C300,43 310,58 400,58",
  "p-controller": "M212,107 C300,107 310,70 400,70",
  "p-agent": "M212,171 C300,171 310,84 400,84",
  "p-showw": "M212,235 C300,235 310,170 400,170",
  "p-autow": "M212,299 C300,299 310,272 400,272",
  "p-manual": "M650,68 C710,68 720,140 745,150",
  "p-show": "M650,170 L728,170",
  "p-auto": "M650,272 C710,272 720,200 745,190",
  "p-frame": "M832,170 L900,170",
  "p-rig": "M1030,170 L1058,170",
};

const SEGMENT_MS = 420;
const reduced = matchMedia("(prefers-reduced-motion: reduce)");

interface Dot {
  id: number;
  route: string[];
  t0: number;
}

/** Sends dots along routes. With reduced motion a dot jumps to the end of each part. */
export function createDots() {
  const [dots, setDots] = createSignal<Dot[]>([]);
  const [tick, setTick] = createSignal(0);
  let raf = 0;
  let seq = 0;
  const loop = () => {
    const now = performance.now();
    setTick(now);
    setDots((ds) => ds.filter((d) => now - d.t0 < d.route.length * SEGMENT_MS + 150));
    raf = dots().length ? requestAnimationFrame(loop) : 0;
  };
  const send = (route: string[]) => {
    setDots((ds) => [...ds, { id: seq++, route, t0: performance.now() }]);
    if (!raf) raf = requestAnimationFrame(loop);
  };
  onCleanup(() => cancelAnimationFrame(raf));
  return { dots, tick, send };
}

export function Flow(props: {
  dots: ReturnType<typeof createDots>;
  /** Parts to light up; everything else dims. Empty: nothing dims. */
  lit: string[];
}) {
  const paths: Record<string, SVGPathElement> = {};
  const isLit = (id: string) => !props.lit.length || props.lit.includes(id);

  const where = (d: Dot) => {
    props.dots.tick();
    const elapsed = performance.now() - d.t0;
    const i = Math.min(d.route.length - 1, Math.floor(elapsed / SEGMENT_MS));
    const el = paths[d.route[i]];
    if (!el) return null;
    const t = reduced.matches ? 1 : Math.min(1, (elapsed - i * SEGMENT_MS) / SEGMENT_MS);
    const p = el.getPointAtLength(el.getTotalLength() * t);
    return { x: p.x, y: p.y };
  };

  const plays = (layer: Box["id"]) => {
    const ids =
      layer === "manual"
        ? store.runningManual().map((b) => b.lookId)
        : layer === "show"
          ? [...store.state.replay, ...store.activeClips().map((c) => c.lookId)]
          : store.autopilotNow().map((l) => l.id);
    const names = [...new Set(ids)].map((id) => lookById(id).name);
    return names.length ? names.join(", ") : "silent";
  };
  const frame = () => Math.floor((store.now() - CLOCK_START) * 44);

  const BoxView = (b: Box) => (
    <g class={`fl-node ${b.layer ? `layer-${b.layer} is-layer` : ""}`} classList={{ "is-dim": !isLit(b.id) }}>
      <rect x={b.x} y={b.y} width={b.w} height={b.h} />
      <text x={b.x + 14} y={b.y + (b.layer ? 30 : 24)} class={b.layer ? "fl-layer-title" : "fl-title"}>
        {b.title}
      </text>
      <Show when={b.sub && !b.layer}>
        <text x={b.x + 14} y={b.y + 44} class="fl-sub">
          {b.sub}
        </text>
      </Show>
      <Show when={b.layer}>
        <text x={b.x + 14} y={b.y + 62} class="fl-plays">
          {plays(b.id)}
        </text>
      </Show>
    </g>
  );

  return (
    <svg class="fl" viewBox="0 0 1200 340" role="img" aria-label="Five writers send commands to three layers. The layers make a frame 44 times a second. The rig turns it into DMX for the output.">
      <defs>
        <marker id="fl-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
          <path d="M0,0 L10,5 L0,10 z" class="fl-arrowhead" />
        </marker>
      </defs>
      <text x="16" y="12" class="fl-col">Five writers</text>
      <text x="400" y="16" class="fl-col">Three layers, top wins</text>

      <For each={Object.entries(PATHS)}>
        {([id, d]) => (
          <path
            ref={(el) => (paths[id] = el)}
            d={d}
            class="fl-path"
            classList={{ "is-dim": !isLit(id) }}
            marker-end="url(#fl-arrow)"
          />
        )}
      </For>

      <For each={WRITERS}>{BoxView}</For>
      <For each={LAYERS}>{BoxView}</For>

      <g class="fl-node fl-frame" classList={{ "is-dim": !isLit("frame") }}>
        <circle cx="788" cy="170" r="44" />
        <text x="788" y="166" text-anchor="middle" class="fl-title">
          Frame
        </text>
        <text x="788" y="186" text-anchor="middle" class="fl-sub">
          44 a second
        </text>
        <text x="788" y="236" text-anchor="middle" class="fl-sub fl-count">
          no. {frame()}
        </text>
      </g>
      {BoxView(RIG)}
      {BoxView(OUT)}

      <For each={props.dots.dots()}>
        {(d) => (
          <Show when={where(d)}>
            {(p) => <circle class="fl-dot" cx={p().x} cy={p().y} r="9" />}
          </Show>
        )}
      </For>
    </svg>
  );
}
