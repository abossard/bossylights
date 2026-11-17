// The Show timeline: ruler, waveform, sections, tracks and the Auto lane.
import { createMemo, For, Show } from "solid-js";
import { hhmm, lookById, store, type ClipTrack, type PlacedSection, type ShowClip, type Song } from "../../fake/store";
import { AutoHeader } from "./AutoHeader";

export type Mode = "edit" | "record" | "run";

const TRACKS: { id: ClipTrack; name: string }[] = [
  { id: "looks", name: "Looks" },
  { id: "movers", name: "Movers" },
  { id: "pixels", name: "Pixels" },
  { id: "laser", name: "Laser" },
  { id: "recorded", name: "Recorded" },
];

export interface TimelineProps {
  song: Song;
  from: number;
  to: number;
  mode: Mode;
  selectedClip: number | null;
  onSelectClip: (id: number | null) => void;
  selectedPick: string | null;
  onSelectPick: (key: string) => void;
}

/** A tiny seeded random, so a song's waveform looks the same every time. */
function seeded(seed: string) {
  let h = [...seed].reduce((a, c) => (a * 31 + c.charCodeAt(0)) >>> 0, 7);
  return () => ((h = (h * 1103515245 + 12345) >>> 0) % 1000) / 1000;
}

export function Timeline(props: TimelineProps) {
  let lanes!: HTMLDivElement;
  const span = () => props.to - props.from;
  const pct = (t: number) => ((t - props.from) / span()) * 100;
  const clampPct = (v: number) => Math.max(0, Math.min(100, v));
  const nowPct = () => clampPct(pct(store.now()));
  const bar = () => (4 * 60) / props.song.bpm;
  const sections = () => store.sectionsBetween(props.from, props.to - 0.001);
  const ticks = () => {
    const out: number[] = [];
    for (let t = Math.ceil(props.from / 300) * 300; t < props.to; t += 300) out.push(t);
    return out;
  };

  const wave = createMemo(() => {
    const rnd = seeded(props.song.id);
    const n = 240;
    const secs = store.sectionsBetween(props.from, props.to - 0.001);
    let d = "";
    for (let i = 0; i < n; i++) {
      const t = props.from + (i / n) * (props.to - props.from);
      const e = secs.find((s) => s.start <= t && t < s.end)?.section.energy ?? 0.3;
      const h = Math.max(2, (0.25 + 0.75 * rnd()) * e * 36);
      d += `M${i + 0.5},${20 - h / 2}v${h}`;
    }
    return { d, n };
  });

  // ---- clip drag (Edit mode) ------------------------------------------
  const drag = (c: ShowClip, how: "move" | "start" | "end") => (e: PointerEvent) => {
    if (props.mode !== "edit" || e.button > 0) return;
    e.stopPropagation();
    props.onSelectClip(c.id);
    const el = e.currentTarget as HTMLElement;
    try {
      el.setPointerCapture(e.pointerId);
    } catch {
      // synthetic pointer: drag still works without capture
    }
    const x0 = e.clientX;
    const w = lanes.getBoundingClientRect().width;
    const s0 = c.start;
    const e0 = c.end;
    const snap = (t: number) => props.from + Math.round((t - props.from) / bar()) * bar();
    const move = (ev: PointerEvent) => {
      const dt = ((ev.clientX - x0) / w) * span();
      if (how === "move") {
        const start = snap(s0 + dt);
        store.moveClip(c.id, start, start + (e0 - s0));
      } else if (how === "start") store.moveClip(c.id, Math.min(snap(s0 + dt), e0 - bar()), e0);
      else store.moveClip(c.id, s0, Math.max(snap(e0 + dt), s0 + bar()));
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

  /** Arrows move a clip by one bar, Shift and arrows change its end. */
  const clipKey = (c: ShowClip) => (e: KeyboardEvent) => {
    if (props.mode !== "edit") return;
    const d = e.key === "ArrowLeft" ? -1 : e.key === "ArrowRight" ? 1 : 0;
    if (!d) return;
    e.preventDefault();
    if (e.shiftKey) store.moveClip(c.id, c.start, Math.max(c.start + bar(), c.end + d * bar()));
    else store.moveClip(c.id, c.start + d * bar(), c.end + d * bar());
  };

  const Clip = (c: ShowClip) => {
    const look = () => lookById(c.lookId);
    const left = () => clampPct(pct(c.start));
    const width = () => clampPct(pct(c.end)) - left();
    return (
      <Show when={width() > 0}>
        <div
          class="tl-clip"
          classList={{
            "is-selected": props.selectedClip === c.id,
            "is-past": c.end <= store.now(),
            "is-editable": props.mode === "edit",
          }}
          style={{ left: `${left()}%`, width: `calc(${width()}% - 2px)` }}
          role="button"
          tabIndex={0}
          aria-pressed={props.selectedClip === c.id}
          aria-label={`${look().name} clip, ${hhmm(c.start)} to ${hhmm(c.end)}${c.origin === "kept" ? ", kept from the autopilot" : ""}`}
          onPointerDown={drag(c, "move")}
          onClick={() => props.onSelectClip(c.id)}
          onKeyDown={clipKey(c)}
        >
          <i class="tl-chip" style={{ background: look().colors[0] }} />
          <span class="tl-clip-name">{look().name}</span>
          <Show when={props.mode === "edit" && props.selectedClip === c.id}>
            <span class="tl-trim tl-trim-start" onPointerDown={drag(c, "start")} aria-hidden="true" />
            <span class="tl-trim tl-trim-end" onPointerDown={drag(c, "end")} aria-hidden="true" />
          </Show>
        </div>
      </Show>
    );
  };

  const Pick = (p: PlacedSection) => {
    const past = () => p.end <= store.now();
    const looks = () => store.autopilotLooks(p);
    const nearNow = () => {
      const keys = sections().filter((s) => !store.covered(s) && s.end > store.now()).slice(0, 2).map((s) => s.key);
      return keys.includes(p.key);
    };
    return (
      <button
        type="button"
        class="tl-pick"
        classList={{ "is-past": past(), "is-pick": nearNow(), "is-selected": props.selectedPick === p.key }}
        style={{ left: `${clampPct(pct(p.start))}%`, width: `calc(${clampPct(pct(p.end)) - clampPct(pct(p.start))}% - 3px)` }}
        aria-pressed={props.selectedPick === p.key}
        onClick={() => props.onSelectPick(p.key)}
      >
        <b class="display">{p.section.title}</b>
        <Show when={nearNow()}>
          <span class="tl-pick-reason">{store.pickReason(p)}</span>
          <span class="tl-pick-looks">{looks().map((l) => l.name).join(", ")}</span>
        </Show>
      </button>
    );
  };

  return (
    <div class="tl" classList={{ "is-run": props.mode === "run", "is-record": props.mode === "record" }}>
      <div class="tl-row tl-ruler">
        <div class="tl-label" />
        <div class="tl-lane" aria-hidden="true">
          <For each={ticks()}>
            {(t) => (
              <span class="tl-tick" style={{ left: `${pct(t)}%` }}>
                {hhmm(t)}
                <small>bar {Math.round((t - props.from) / bar()) + 1}</small>
              </span>
            )}
          </For>
        </div>
      </div>
      <Show when={props.mode !== "run"}>
        <div class="tl-row tl-wave">
          <div class="tl-label mute">Waveform</div>
          <div class="tl-lane">
            <svg viewBox={`0 0 ${wave().n} 40`} preserveAspectRatio="none" aria-label={`Waveform of ${props.song.title}`} role="img">
              <path d={wave().d} vector-effect="non-scaling-stroke" />
            </svg>
          </div>
        </div>
      </Show>
      <div class="tl-row tl-sections">
        <div class="tl-label mute">Sections</div>
        <div class="tl-lane">
          <For each={sections()}>
            {(p) => (
              <span class="tl-section" style={{ left: `${pct(p.start)}%`, width: `calc(${pct(p.end) - pct(p.start)}% - 2px)` }}>
                {p.section.title}
              </span>
            )}
          </For>
        </div>
      </div>
      <div ref={lanes} class="tl-tracks layer-show">
        <For each={props.mode === "run" ? [] : TRACKS}>
          {(tr) => (
            <div class="tl-row tl-track" classList={{ "is-recorded": tr.id === "recorded" }}>
              <div class="tl-label">{tr.name}</div>
              <div class="tl-lane">
                <For each={store.state.clips.filter((c) => c.track === tr.id && c.end > props.from && c.start < props.to)}>
                  {(c) => Clip(c)}
                </For>
                <Show when={tr.id === "recorded"}>
                  <For each={store.state.events.filter((ev) => ev.at >= props.from && ev.at < props.to && ev.action !== "stop")}>
                    {(ev) => (
                      <span class="tl-event" style={{ left: `${pct(ev.at)}%` }} title={lookById(ev.lookId).name}>
                        <i style={{ background: lookById(ev.lookId).colors[0] }} />
                        {lookById(ev.lookId).name}
                      </span>
                    )}
                  </For>
                </Show>
              </div>
            </div>
          )}
        </For>
      </div>
      <Show when={props.mode === "run"}>
        <div class="tl-row tl-track layer-show">
          <div class="tl-label">Show</div>
          <div class="tl-lane">
            <For each={store.state.clips.filter((c) => c.track !== "recorded" && c.end > props.from && c.start < props.to)}>
              {(c) => Clip(c)}
            </For>
          </div>
        </div>
      </Show>
      <div class="tl-row tl-auto layer-autopilot">
        <div class="tl-label">
          <AutoHeader />
        </div>
        <div class="tl-lane">
          <For each={sections().filter((p) => !store.covered(p))}>{(p) => Pick(p)}</For>
        </div>
      </div>
      <div class="tl-now-layer" aria-hidden="true">
        <span class="now-flag tl-flag" style={{ left: `clamp(30px, ${nowPct()}%, calc(100% - 36px))` }}>
          Now {hhmm(store.now())}
        </span>
        <span class="tl-now" style={{ left: `${nowPct()}%` }} />
      </div>
    </div>
  );
}
