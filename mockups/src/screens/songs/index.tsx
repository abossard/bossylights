import { createMemo, createSignal, For, Show } from "solid-js";
import {
  LIBRARY_ONLY,
  LOOKS,
  lookById,
  mmss,
  SONGS,
  store,
  type LookKind,
  type Song,
} from "../../fake/store";
import { Button, EnergyCurve, List, Region, Segmented } from "../../kit";
import type { Issue } from "../../shell/registry";
import "./songs.css";

export const id = "songs";
export const title = "Songs";
export const order = 2;
export const nowStrip = false;
export const issues: Issue[] = [
  { number: 96, title: "Song Director" },
  { number: 47, title: "Song analysis: grid, sections, energy" },
  { number: 52, title: "VirtualDJ link" },
  { number: 85, title: "Show timeline with waveform" },
  { number: 94, title: "Record what you busk" },
  { number: 95, title: "Follow timecode" },
  { number: 99, title: "DJ set shows" },
  { number: 70, title: "Cue lists" },
  { number: 68, title: "Looks generated from your rig" },
];

const LIBRARY: Song[] = [...SONGS, ...LIBRARY_ONLY];

const GROUPS: { kind: LookKind; name: string }[] = [
  { kind: "wash", name: "Wash" },
  { kind: "movement", name: "Movement" },
  { kind: "strobe", name: "Strobe" },
  { kind: "pixels", name: "Pixels" },
];

const ANALYSIS: Record<Song["analysis"], string> = { analysed: "Analysed", queued: "Queued", "needs-review": "Needs review" };

type Filter = "show" | "auto" | "today" | "review";
const FILTERS: { id: Filter; name: string }[] = [
  { id: "show", name: "Has a show" },
  { id: "auto", name: "No show yet" },
  { id: "today", name: "Has recordings" },
  { id: "review", name: "Needs review" },
];

/** A tiny seeded random, so a song's waveform looks the same every time. */
function seeded(seed: string) {
  let h = [...seed].reduce((a, c) => (a * 31 + c.charCodeAt(0)) >>> 0, 7);
  return () => ((h = (h * 1103515245 + 12345) >>> 0) % 1000) / 1000;
}

export function Screen() {
  const playing = () => store.currentSong();
  // a jump from the Looks screen ("used by: Glass Hours, Drop") lands on its song and section
  const jump = store.state.jump;
  const [songId, setSongId] = createSignal(jump.song ?? playing()?.song.id ?? SONGS[0].id);
  const song = () => LIBRARY.find((s) => s.id === songId())!;
  const secs = () => store.sectionsOf(song());
  const len = () => store.songLength(song());
  const offsets = () => {
    let t = 0;
    return secs().map((s) => ({ start: t, end: (t += s.length) }));
  };
  const analysis = () => store.analysisOf(song());
  const notReady = () => analysis() === "queued";
  const isPlaying = () => playing()?.song.id === song().id;
  const liveEdit = () => isPlaying() && store.state.night.live;

  // ---- library: search and one filter
  const [query, setQuery] = createSignal("");
  const [filter, setFilter] = createSignal<Filter | null>(null);
  const hasShow = (s: Song) => store.sectionsOf(s).some((_, i) => store.sectionLooksOf(s, i).length);
  const shown = () =>
    LIBRARY.filter((s) => !query().trim() || `${s.title} ${s.artist}`.toLowerCase().includes(query().trim().toLowerCase())).filter((s) => {
      const f = filter();
      if (f === "show") return hasShow(s);
      if (f === "auto") return !hasShow(s);
      if (f === "today") return store.recordedOf(s).passes.length > 0;
      if (f === "review") return store.analysisOf(s) === "needs-review";
      return true;
    });

  // ---- the selected section
  const [chosen, setChosen] = createSignal<Record<string, number>>(jump.song && jump.section !== undefined ? { [jump.song]: jump.section } : {});
  const playingIndex = () => {
    if (!isPlaying()) return -1;
    const pos = store.songPos();
    return offsets().findIndex((o) => o.start <= pos && pos < o.end);
  };
  const selected = () => {
    const c = chosen()[songId()];
    if (c !== undefined && c < secs().length) return c;
    if (playingIndex() >= 0) return playingIndex();
    return 0;
  };
  const select = (i: number) => setChosen({ ...chosen(), [songId()]: i });

  // ---- listening from a section: a local playhead
  const [listen, setListen] = createSignal<{ songId: string; from: number; at: number } | null>(null);
  const listenPos = () => {
    const l = listen();
    if (!l || l.songId !== songId()) return null;
    store.now();
    const p = l.from + (performance.now() - l.at) / 1000;
    return p < len() ? p : null;
  };
  const nowPos = () => listenPos() ?? (isPlaying() ? store.songPos() : null);
  const pct = (t: number) => (t / len()) * 100;

  const [follows, setFollows] = createSignal<"deck1" | "deck2" | "timecode">("deck1");
  const [message, setMessage] = createSignal("");

  const add = (i: number, lookId: string) => {
    const ids = store.sectionLooksOf(song(), i);
    if (ids.includes(lookId)) return setMessage(`${lookById(lookId).name} is already in ${secs()[i].title}.`);
    store.setSectionLooks(song(), i, [...ids, lookId], `Add ${lookById(lookId).name}`);
    setMessage(`Added ${lookById(lookId).name} to ${secs()[i].title}.`);
  };
  const remove = (i: number, lookId: string) =>
    store.setSectionLooks(song(), i, store.sectionLooksOf(song(), i).filter((x) => x !== lookId), `Remove ${lookById(lookId).name}`);

  // ---- section edges: drag on the ruler
  let ruler!: HTMLDivElement;
  const dragEdge = (i: number) => (e: PointerEvent) => {
    if (e.button > 0) return;
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    try {
      el.setPointerCapture(e.pointerId);
    } catch {
      // synthetic pointer: drag still works
    }
    const x0 = e.clientX;
    const w = ruler.getBoundingClientRect().width;
    const edge0 = offsets()[i].end;
    const move = (ev: PointerEvent) => {
      const target = edge0 + ((ev.clientX - x0) / w) * len();
      store.moveSectionEdge(song(), i, target - offsets()[i].end);
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

  const rec = () => store.recordedOf(song());
  /** Passes that are done: "Undo last pass" takes the newest of them. */
  const donePasses = () => (rec().writing ? rec().passes.length - 1 : rec().passes.length);

  const wave = createMemo(() => {
    const s = song();
    const rnd = seeded(s.id);
    const n = 200;
    const length = store.songLength(s);
    const offs = offsets();
    let d = "";
    for (let i = 0; i < n; i++) {
      const t = (i / n) * length;
      const e = store.sectionsOf(s)[offs.findIndex((o) => o.start <= t && t < o.end)]?.energy ?? 0.3;
      const h = Math.max(2, (0.25 + 0.75 * rnd()) * e * 44);
      d += `M${i + 0.5},${24 - h / 2}v${h}`;
    }
    return d;
  });

  const [renaming, setRenaming] = createSignal(false);

  return (
    <div class="songs-screen">
      <Region id="library" title="Song library" class="songs-lib" aside={<span class="mute">{shown().length} of {LIBRARY.length}</span>}>
        <input type="text" placeholder="Search title or artist" aria-label="Search songs" value={query()} onInput={(e) => setQuery(e.currentTarget.value)} />
        <div class="songs-filters" role="group" aria-label="Filter">
          <For each={FILTERS}>
            {(f) => (
              <button type="button" class="songs-filter" aria-pressed={filter() === f.id} onClick={() => setFilter(filter() === f.id ? null : f.id)}>
                {f.name}
              </button>
            )}
          </For>
        </div>
        <div class="songs-lib-list">
          <List
            label="Song library"
            selected={songId()}
            onSelect={(id) => {
              setSongId(id);
              setMessage("");
              setRenaming(false);
            }}
            empty="No song matches. Clear the search or the filter."
            items={shown().map((s) => {
              const a = store.analysisOf(s);
              return {
                id: s.id,
                title: (
                  <>
                    {s.title}
                    <Show when={playing()?.song.id === s.id}>
                      {" "}
                      <span class="songs-flag">Playing</span>
                    </Show>
                  </>
                ),
                sub: a === "queued" ? `${s.artist} · not analysed yet` : `${s.artist} · ${s.bpm} BPM · ${Math.round(s.confidence * 100)}% sure`,
                meta: (
                  <span class="songs-metas">
                    <span class={`songs-state is-${a}`}>{ANALYSIS[a]}</span>
                    <span class="songs-has">{hasShow(s) ? "Show" : "No show yet"}</span>
                  </span>
                ),
              };
            })}
          />
        </div>
        <p class="mute songs-sample">Songs are sample data.</p>
      </Region>

      <div class="songs-centre">
        <Region id="song" title="Song" bare class="songs-song">
          <header class="songs-head">
            <div class="songs-title">
              <h2 class="display">
                {song().title}
                <Show when={liveEdit()}>
                  <span class="songs-liveedit" title="Every change is on the lights at once">
                    <i aria-hidden="true" />
                    Live edit
                  </span>
                </Show>
              </h2>
              <span class="mute">
                {song().artist} · {notReady() ? "tempo not known yet" : `${song().bpm} BPM`} · {mmss(len())}
                <Show when={isPlaying()}> · playing now</Show>
              </span>
            </div>
            <div class="songs-actions">
              <Segmented
                label="Follows"
                value={follows()}
                onChange={setFollows}
                options={[
                  { value: "deck1", label: "Deck 1" },
                  { value: "deck2", label: "Deck 2" },
                  { value: "timecode", label: "Timecode" },
                ]}
              />
              <Show
                when={listenPos() === null}
                fallback={
                  <Button pressed onClick={() => setListen(null)}>
                    Stop listening
                  </Button>
                }
              >
                <Button
                  disabled={notReady()}
                  reason="Not analysed yet"
                  onClick={() => setListen({ songId: songId(), from: offsets()[selected()].start, at: performance.now() })}
                >
                  Play from {secs()[selected()]?.title}
                </Button>
              </Show>
            </div>
          </header>

          <Show when={analysis() === "needs-review"}>
            <div class="songs-banner" role="status">
              <span>
                <strong>The analysis is only {Math.round(song().confidence * 100)}% sure about this song.</strong>{" "}
                <span class="mute">Check the sections against the music before the show.</span>
              </span>
              <Button onClick={() => store.markReviewed(song())}>Sections look right</Button>
            </div>
          </Show>

          <Show
            when={!notReady()}
            fallback={
              <div class="songs-wait">
                <p class="display">Queued for analysis</p>
                <p class="mute">Its tempo, sections and energy appear here when the analysis is done. Until then the autopilot plays it.</p>
              </div>
            }
          >
            <div class="songs-tl">
              {/* waveform and section ruler */}
              <div class="songs-tl-row songs-tl-wave">
                <span class="songs-tl-label mute">Song</span>
                <svg class="songs-wave" viewBox="0 0 200 48" preserveAspectRatio="none" role="img" aria-label={`Waveform of ${song().title}`}>
                  <path d={wave()} vector-effect="non-scaling-stroke" />
                </svg>
              </div>
              <div class="songs-tl-row">
                <span class="songs-tl-label mute">Sections</span>
                <div ref={ruler} class="songs-sections" role="group" aria-label="Sections">
                  <For each={secs()}>
                    {(sec, i) => (
                      <button
                        type="button"
                        class="songs-sec"
                        classList={{ "is-selected": selected() === i(), "is-empty": !store.sectionLooksOf(song(), i()).length }}
                        style={{ left: `${pct(offsets()[i()].start)}%`, width: `calc(${pct(sec.length)}% - 2px)` }}
                        aria-pressed={selected() === i()}
                        onClick={() => select(i())}
                      >
                        <b class="display">{sec.title}</b>
                        <span>{mmss(offsets()[i()].start)}</span>
                      </button>
                    )}
                  </For>
                  <For each={secs().slice(0, -1)}>
                    {(sec, i) => (
                      <span
                        class="songs-edge"
                        style={{ left: `${pct(offsets()[i()].end)}%` }}
                        role="slider"
                        tabIndex={0}
                        aria-label={`Edge between ${sec.title} and ${secs()[i() + 1].title}, at ${mmss(offsets()[i()].end)}`}
                        aria-valuenow={Math.round(offsets()[i()].end)}
                        onPointerDown={dragEdge(i())}
                        onKeyDown={(e) => {
                          const d = e.key === "ArrowLeft" ? -4 : e.key === "ArrowRight" ? 4 : 0;
                          if (!d) return;
                          e.preventDefault();
                          store.moveSectionEdge(song(), i(), d);
                        }}
                      />
                    )}
                  </For>
                </div>
              </div>

              {/* what was recorded: one row, every pass adds to it, red while it writes */}
              <div class="songs-tl-row songs-take" classList={{ "is-recording": rec().writing, "is-muted": rec().muted }}>
                <div class="songs-tl-label">
                  <b>
                    <Show when={rec().writing}>
                      <i class="songs-rec-dot" aria-hidden="true" />
                    </Show>
                    Recorded
                    <span class="songs-passes">{rec().passes.length ? `${rec().passes.length} ${rec().passes.length === 1 ? "pass" : "passes"}${rec().writing ? ", writing" : ""}` : "nothing yet"}</span>
                  </b>
                  <span class="songs-take-actions">
                    <button type="button" aria-pressed={rec().muted} disabled={!rec().passes.length} onClick={() => store.setRecordedMuted(song(), !rec().muted)}>
                      {rec().muted ? "Muted" : "Mute"}
                    </button>
                    <button type="button" disabled={!donePasses()} onClick={() => store.undoLastPass(song())}>
                      Undo last pass
                    </button>
                    <button type="button" disabled={!rec().passes.length} onClick={() => store.clearRecorded(song())}>
                      Clear
                    </button>
                  </span>
                </div>
                <div class="songs-track">
                  <For each={rec().passes}>
                    {(pass, pi) => (
                      <>
                        <For each={store.recBlocks(pass.events, len(), rec().writing && pi() === rec().passes.length - 1, store.songPos())}>
                          {(b) => (
                            <span
                              class="songs-rec-block"
                              classList={{ "is-writing": rec().writing && pi() === rec().passes.length - 1 }}
                              style={{ left: `${pct(b.start)}%`, width: `max(6px, ${pct(b.end - b.start)}%)` }}
                              title={lookById(b.lookId).name}
                            >
                              <i style={{ background: lookById(b.lookId).colors[0] }} />
                              {lookById(b.lookId).name}
                            </span>
                          )}
                        </For>
                        <For each={pass.events.filter((ev) => ev.kind === "fader")}>
                          {(ev) => (
                            <span class="songs-fader-mark" style={{ left: `${pct(ev.at)}%` }}>
                              {ev.fader} {ev.value}
                            </span>
                          )}
                        </For>
                      </>
                    )}
                  </For>
                  <Show when={!rec().passes.length}>
                    <span class="songs-rec-empty">Turn Recording on and play this song: what you do on the Console lands here.</span>
                  </Show>
                </div>
              </div>
              <div class="songs-tl-row songs-take" classList={{ "is-muted": !!store.state.showMuted[song().id] }}>
                <div class="songs-tl-label">
                  <b>Show</b>
                  <span class="songs-take-actions">
                    <button type="button" aria-pressed={!!store.state.showMuted[song().id]} onClick={() => store.setShowMuted(song(), !store.state.showMuted[song().id])}>
                      {store.state.showMuted[song().id] ? "Muted" : "Mute"}
                    </button>
                  </span>
                </div>
                <div class="songs-track">
                  <For each={secs()}>
                    {(_, i) => (
                      <Show when={store.sectionLooksOf(song(), i()).length}>
                        <button
                          type="button"
                          class="songs-show-block"
                          classList={{ "is-selected": selected() === i() }}
                          style={{ left: `${pct(offsets()[i()].start)}%`, width: `calc(${pct(secs()[i()].length)}% - 2px)` }}
                          onClick={() => select(i())}
                        >
                          <span class="songs-chips">
                            <For each={store.sectionLooksOf(song(), i())}>{(id) => <i style={{ background: lookById(id).colors[0] }} />}</For>
                          </span>
                          <span class="songs-show-names">{store.sectionLooksOf(song(), i()).map((id) => lookById(id).name).join(", ")}</span>
                        </button>
                      </Show>
                    )}
                  </For>
                </div>
              </div>

              <Show when={nowPos() !== null}>
                <span class="songs-now" style={{ left: `calc(var(--tl-label) + (100% - var(--tl-label)) * ${nowPos()! / len()})` }} aria-hidden="true" />
                <span class="now-flag songs-now-flag" style={{ left: `calc(var(--tl-label) + (100% - var(--tl-label)) * ${nowPos()! / len()})` }}>
                  {listenPos() !== null ? "Listening" : "Now"} {mmss(nowPos()!)}
                </span>
              </Show>
            </div>

            {/* the selected section: its looks, and how to change the section */}
            <section class="songs-section-edit" aria-label="Selected section">
              <header>
                <Show
                  when={renaming()}
                  fallback={
                    <h3 class="display">
                      {secs()[selected()].title} <span class="mute">{mmss(offsets()[selected()].start)} to {mmss(offsets()[selected()].end)}</span>
                    </h3>
                  }
                >
                  <form
                    class="songs-rename"
                    onSubmit={(e) => {
                      e.preventDefault();
                      const v = (e.currentTarget.elements.namedItem("title") as HTMLInputElement).value;
                      store.renameSection(song(), selected(), v);
                      setRenaming(false);
                    }}
                  >
                    <input type="text" name="title" aria-label="Section name" value={secs()[selected()].title} ref={(el) => queueMicrotask(() => el.select())} />
                    <Button type="submit" variant="primary">
                      Save
                    </Button>
                  </form>
                </Show>
                <span class="songs-sec-actions">
                  <Button onClick={() => setRenaming(!renaming())}>{renaming() ? "Cancel" : "Rename"}</Button>
                  <Button onClick={() => store.splitSection(song(), selected())}>Split</Button>
                  <Button disabled={selected() >= secs().length - 1} reason="It is the last one" onClick={() => store.mergeSections(song(), selected())}>
                    Merge with next
                  </Button>
                  <Button
                    disabled={secs().every((_, i) => store.sectionLooksOf(song(), i).length)}
                    reason="Every section has looks"
                    onClick={() => {
                      store.fillEmptySections(song());
                      setMessage("Filled the empty sections: the autopilot picked once, with its settings from Looks.");
                    }}
                  >
                    Fill empty sections
                  </Button>
                </span>
              </header>
              <ul class="songs-looks-in">
                <For
                  each={store.sectionLooksOf(song(), selected())}
                  fallback={<li class="songs-empty">No looks in this section. Tap a look on the right to add one, or press Fill empty sections.</li>}
                >
                  {(lookId) => (
                    <li class="songs-look">
                      <i style={{ background: lookById(lookId).colors[0] }} />
                      <span>{lookById(lookId).name}</span>
                      <button type="button" class="songs-x" aria-label={`Remove ${lookById(lookId).name}`} onClick={() => remove(selected(), lookId)}>
                        <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
                          <path d="M2 2l8 8M10 2l-8 8" stroke="currentColor" stroke-width="2.5" />
                        </svg>
                      </button>
                    </li>
                  )}
                </For>
              </ul>
              <p class="songs-msg" role="status">
                {message()}
              </p>
            </section>
          </Show>
        </Region>

        <Region id="energy" title="Energy" class="songs-energy" aside={<span class="mute">How hard the song hits, section by section</span>}>
          <Show when={!notReady()} fallback={<p class="mute">Appears after the analysis.</p>}>
            <EnergyCurve
              label={`Energy of ${song().title}`}
              from={0}
              to={len()}
              now={nowPos() ?? -1}
              spans={secs().map((s, i) => ({ ...offsets()[i], energy: s.energy, title: s.title }))}
            />
          </Show>
        </Region>
      </div>

      <Region id="looks" title="Looks to add" class="songs-looks" aside={<span class="mute">to {notReady() ? "nothing yet" : secs()[selected()]?.title}</span>}>
        <div class="songs-looklist">
          <For each={GROUPS}>
            {(g) => (
              <section>
                <h3 class="songs-group">{g.name}</h3>
                <ul>
                  <For each={LOOKS.filter((l) => l.kind === g.kind)}>
                    {(l) => (
                      <li>
                        <button
                          type="button"
                          class="songs-addlook"
                          disabled={notReady()}
                          onClick={() => add(selected(), l.id)}
                          aria-label={`Add ${l.name} to ${secs()[selected()]?.title}`}
                        >
                          <i>
                            <For each={l.colors}>{(c) => <span style={{ background: c }} />}</For>
                          </i>
                          {l.name}
                        </button>
                      </li>
                    )}
                  </For>
                </ul>
              </section>
            )}
          </For>
        </div>
      </Region>
    </div>
  );
}
