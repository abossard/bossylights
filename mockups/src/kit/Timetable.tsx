import { For, Show, type JSX } from "solid-js";
import type { Lane, LaneBlock } from "../fake/store";

export interface TimetableProps {
  /** Window start and end, in the same unit as now (show-clock seconds). */
  from: number;
  to: number;
  now: number;
  lanes: Lane[];
  /** Ruler tick spacing, same unit. */
  tickEvery: number;
  /** Ruler and flag text for a time. */
  format: (t: number) => string;
  /** CSS length of one lane. Default 112px. */
  laneHeight?: string;
  /** CSS length of the label column. Default 124px. */
  labelWidth?: string;
  /** Controls for a lane header, e.g. the autopilot's on/off and style. They replace the ON/OFF chip. */
  laneControls?: (lane: Lane) => JSX.Element | undefined;
  /** Hide lane labels' status lines (single-lane rulers). */
  plain?: boolean;
  class?: string;
}

/**
 * Lanes, a time ruler and the NOW line with its flag. Past blocks are
 * outlined, present and future blocks are filled, a block that runs with no
 * end grows to the NOW line and shows a dashed tail.
 */
export function Timetable(props: TimetableProps) {
  const span = () => props.to - props.from;
  const pct = (t: number) => ((t - props.from) / span()) * 100;
  const clampPct = (v: number) => Math.max(0, Math.min(100, v));
  const ticks = () => {
    const out: number[] = [];
    for (let t = Math.ceil(props.from / props.tickEvery) * props.tickEvery; t < props.to; t += props.tickEvery) out.push(t);
    return out;
  };
  const nowPct = () => clampPct(pct(props.now));

  /** Blocks that run at the same time stack in tracks inside the lane. */
  const pack = (blocks: LaneBlock[]) => {
    const ends: number[] = [];
    const track = new Map<string, number>();
    for (const b of [...blocks].sort((x, y) => x.start - y.start)) {
      let i = ends.findIndex((e) => e <= b.start);
      if (i < 0) i = ends.push(0) - 1;
      ends[i] = b.end ?? Infinity;
      track.set(b.id, i);
    }
    return { track, count: Math.max(1, ends.length) };
  };

  const Block = (b: LaneBlock, pos: () => { track: number; count: number }) => {
    const end = () => b.end ?? props.now;
    const left = () => clampPct(pct(b.start));
    const right = () => clampPct(pct(end()));
    const width = () => right() - left();
    const past = () => b.end !== null && b.end <= props.now;
    const size = () => (width() < 7 ? "tt-b-xs" : width() < 12 ? "tt-b-sm" : "");
    return (
      <>
        <Show when={width() > 0}>
          <div
            class={`tt-block ${size()}`}
            classList={{ "is-past": past(), "is-stacked": pos().count > 1 }}
            style={{ left: `${left()}%`, width: `calc(${width()}% - 4px)`, "--track": pos().track, "--tracks": pos().count }}
          >
            <b class="display">{b.title}</b>
            <For each={b.sub?.split("\n") ?? []}>{(line) => <i>{line}</i>}</For>
          </div>
        </Show>
        <Show when={b.end === null && b.tail}>
          <div
            class="tt-tail"
            classList={{ "is-stacked": pos().count > 1 }}
            style={{ left: `${nowPct()}%`, width: `${Math.min(9, 100 - nowPct())}%`, "--track": pos().track, "--tracks": pos().count }}
          >
            {b.tail}
          </div>
        </Show>
      </>
    );
  };

  return (
    <div
      class={`tt ${props.plain ? "tt-plain" : ""} ${props.class ?? ""}`}
      style={{
        "--tt-lane": props.laneHeight ?? "112px",
        "--tt-label": props.labelWidth ?? "124px",
        "--tt-lanes": props.lanes.length,
      }}
    >
      <div />
      <div class="tt-ruler" aria-hidden="true">
        <For each={ticks()}>{(t) => <span style={{ left: `${pct(t)}%` }}>{props.format(t)}</span>}</For>
      </div>
      <For each={props.lanes}>
        {(lane) => (
          <>
            <div class={`tt-label layer-${lane.layer}`} classList={{ "has-controls": !!props.laneControls?.(lane) }}>
              <h3 class="display">{lane.title}</h3>
              <Show
                when={props.laneControls?.(lane)}
                fallback={
                  <Show when={!props.plain}>
                    <p>
                      <span class="tt-state" classList={{ "is-on": lane.on }}>{lane.on ? "ON" : "OFF"}</span>
                      <span class="mute">{lane.status}</span>
                    </p>
                  </Show>
                }
              >
                {(controls) => (
                  <>
                    <div class="tt-controls">{controls()}</div>
                    <p class="tt-status mute">{lane.status}</p>
                  </>
                )}
              </Show>
            </div>
            <div class={`tt-lane layer-${lane.layer}`} classList={{ "is-off": !lane.on }} aria-label={`${lane.title} lane`}>
              {(() => {
                const packed = () => pack(lane.blocks);
                return (
                  <For each={lane.blocks}>
                    {(b) => Block(b, () => ({ track: packed().track.get(b.id) ?? 0, count: packed().count }))}
                  </For>
                );
              })()}
              <Show when={!lane.blocks.length && lane.empty}>
                <div class="tt-empty">{lane.empty}</div>
              </Show>
            </div>
          </>
        )}
      </For>
      <div class="tt-grid" aria-hidden="true">
        <For each={ticks()}>{(t) => <i style={{ left: `${pct(t)}%` }} />}</For>
      </div>
      <div class="tt-over" aria-hidden="true">
        <span class="now-flag tt-flag" style={{ left: `clamp(32px, ${nowPct()}%, calc(100% - 40px))` }}>
          Now {props.format(props.now)}
        </span>
        <span class="tt-now" style={{ left: `${nowPct()}%` }} />
      </div>
    </div>
  );
}

/**
 * A time window that holds still and jumps in steps, so the NOW line walks
 * across it at clock speed. lead: how much past to show at least.
 */
export function stepWindow(now: number, lead: number, span: number, step: number) {
  const from = Math.floor((now - lead) / step) * step;
  return { from, to: from + span };
}
