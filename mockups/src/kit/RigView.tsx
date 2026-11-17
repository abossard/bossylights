import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import type { Fixture, Layer } from "../fake/data";

export interface FixtureLook {
  /** Colour it outputs, or null when it has nothing to do. */
  color: string | null;
  /** 0..1 after master and faders */
  level: number;
}

export interface RigZone {
  label: string;
  layer: Layer;
  fixtureIds: string[];
}

export interface RigViewProps {
  fixtures: Fixture[];
  /** What each fixture outputs now. */
  lookOf: (f: Fixture) => FixtureLook;
  /** Outlines with a label, e.g. what the Manual layer covers. */
  zones?: RigZone[];
  /** Makes fixtures pressable (select in Looks, Patch). */
  onFixture?: (f: Fixture) => void;
  selected?: string[];
  label?: string;
  class?: string;
}

const PLATE_CHAR = 7;
const plateW = (s: string) => s.length * PLATE_CHAR + 12;

/** The rig from above: fixtures with name plates and the colours they show now. */
export function RigView(props: RigViewProps) {
  /**
   * A zone's box. The top and sides are in %, the bottom sits a fixed number of
   * pixels under the lowest fixture so its name plate always stays inside.
   */
  const zoneBox = (z: RigZone) => {
    const fs = props.fixtures.filter((f) => z.fixtureIds.includes(f.id));
    const xs = fs.map((f) => f.x);
    const ys = fs.map((f) => f.y);
    return {
      x0: (Math.min(...xs) - 0.04) * 100,
      x1: (Math.max(...xs) + 0.04) * 100,
      yTop: Math.min(...ys) * 100,
      yLow: Math.max(...ys) * 100,
    };
  };
  const ZONE_PAD = 40;
  const ZONE_TOP = 38;


  // Narrow rigs show short plates ("L1", "3") so plates never overlap.
  let svg!: SVGSVGElement;
  const [width, setWidth] = createSignal(800);
  onMount(() => {
    const ro = new ResizeObserver(([e]) => setWidth(e.contentRect.width));
    ro.observe(svg);
    onCleanup(() => ro.disconnect());
  });
  /** True when the plates of this kind would touch at the current width. */
  const compact = (kind: Fixture["kind"]) => {
    const fs = props.fixtures.filter((f) => f.kind === kind);
    const xs = fs.map((f) => f.x).sort((a, b) => a - b);
    const gap = Math.min(...xs.slice(1).map((x, i) => x - xs[i]));
    return gap * width() < Math.max(...fs.map((f) => plateW(f.name))) + 4;
  };
  const plateText = (f: Fixture) => {
    if (f.kind === "laser") return props.lookOf(f).level === 0 && !compact("mover") ? `${f.name}, off` : f.name;
    if ((f.kind === "mover" || f.kind === "par") && compact(f.kind)) return f.name.split(" ").pop()!;
    return f.name;
  };

  return (
    <svg ref={svg} class={`rig ${props.class ?? ""}`} role="img" aria-label={props.label ?? "Rig, top view, with the colours it shows now"}>
      <line class="rig-front" x1="2%" x2="98%" y1="94%" y2="94%" />
      <text class="rig-front-label" x="50%" y="94%" dy="-6" text-anchor="middle">
        Front of stage
      </text>

      <For each={props.zones ?? []}>
        {(z) => {
          const b = () => zoneBox(z);
          return (
            <g class={`rig-zone layer-${z.layer}`}>
              <line x1={`${b().x0}%`} x2={`${b().x0}%`} y1={`${b().yTop}%`} y2={`${b().yLow}%`} />
              <line x1={`${b().x1}%`} x2={`${b().x1}%`} y1={`${b().yTop}%`} y2={`${b().yLow}%`} />
              <svg y={`${b().yTop}%`} overflow="visible">
                <line x1={`${b().x0}%`} x2={`${b().x0}%`} y1={-ZONE_TOP} y2="0" />
                <line x1={`${b().x1}%`} x2={`${b().x1}%`} y1={-ZONE_TOP} y2="0" />
                <line x1={`${b().x0}%`} x2={`${b().x1}%`} y1={-ZONE_TOP} y2={-ZONE_TOP} />
              </svg>
              <svg y={`${b().yLow}%`} overflow="visible">
                <line x1={`${b().x0}%`} x2={`${b().x0}%`} y1="0" y2={ZONE_PAD} />
                <line x1={`${b().x1}%`} x2={`${b().x1}%`} y1="0" y2={ZONE_PAD} />
                <line x1={`${b().x0}%`} x2={`${b().x1}%`} y1={ZONE_PAD} y2={ZONE_PAD} />
              </svg>
              <svg x={`${b().x0}%`} y={`${b().yTop}%`} overflow="visible">
                <rect class="rig-zone-chip" y={-ZONE_TOP} width={plateW(z.label) + 4} height="20" />
                <text x="8" y={-ZONE_TOP + 14}>{z.label}</text>
              </svg>
            </g>
          );
        }}
      </For>

      <For each={props.fixtures}>
        {(f) => {
          const look = () => props.lookOf(f);
          const lit = () => look().color !== null && look().level > 0;
          const opacity = () => (lit() ? 0.55 + 0.45 * look().level : 0);
          const selected = () => props.selected?.includes(f.id) ?? false;
          const press = () => props.onFixture?.(f);
          const plateY = f.kind === "bar" ? 0 : f.kind === "mover" ? 34 : 26;
          return (
            <svg x={`${f.x * 100}%`} y={`${f.y * 100}%`} overflow="visible">
              <g
                class={`rig-fx rig-${f.kind}`}
                classList={{ "is-selected": selected(), "is-pressable": !!props.onFixture }}
                role={props.onFixture ? "button" : undefined}
                tabIndex={props.onFixture ? 0 : undefined}
                aria-label={props.onFixture ? f.name : undefined}
                aria-pressed={props.onFixture ? selected() : undefined}
                onClick={press}
                onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), press())}
              >
                <Show when={props.onFixture}>
                  <rect class="rig-hit" x="-24" y="-24" width="48" height="48" />
                </Show>
                <Show when={f.kind === "mover"}>
                  <circle class="rig-body" r="20" />
                  <circle class="rig-light" r="18" fill={look().color ?? "none"} fill-opacity={opacity()} />
                </Show>
                <Show when={f.kind === "par"}>
                  <circle class="rig-body" r="13" />
                  <circle class="rig-light" r="11.5" fill={look().color ?? "none"} fill-opacity={opacity()} />
                </Show>
                <Show when={f.kind === "laser"}>
                  <rect class="rig-body rig-laser-body" x="-14" y="-14" width="28" height="28" />
                  <rect class="rig-light" x="-12" y="-12" width="24" height="24" fill={look().color ?? "none"} fill-opacity={opacity()} />
                </Show>
                <Show when={f.kind === "bar"}>
                  <rect class="rig-body" x="-13" y="-22%" width="26" height="44%" />
                  <rect class="rig-light" x="-11" y="-22%" width="22" height="44%" fill={look().color ?? "none"} fill-opacity={opacity()} />
                </Show>
                <Show when={selected()}>
                  <circle class="rig-sel" r={f.kind === "mover" ? 27 : 20} />
                </Show>
                <Show
                  when={f.kind === "bar"}
                  fallback={
                    <>
                      <rect class="rig-plate" x={-plateW(plateText(f)) / 2} y={plateY - 13} width={plateW(plateText(f))} height="19" />
                      <text class="rig-plate-text" y={plateY} text-anchor="middle">{plateText(f)}</text>
                    </>
                  }
                >
                  <svg y="22%" overflow="visible">
                    <rect class="rig-plate" x={-plateW(f.name) / 2} y="6" width={plateW(f.name)} height="19" />
                    <text class="rig-plate-text" y="19" text-anchor="middle">{f.name}</text>
                  </svg>
                </Show>
              </g>
            </svg>
          );
        }}
      </For>
    </svg>
  );
}
