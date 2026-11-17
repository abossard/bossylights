import { createMemo, createSignal, For, Show } from "solid-js";
import { LIBRARY_ONLY, LOOKS, lookById, mmss, SONGS, store, type LookKind, type Song } from "../../fake/store";
import { Button, EnergyCurve, List, Region } from "../../kit";
import type { Issue } from "../../shell/registry";
import "./songs.css";

export const id = "songs";
export const title = "Songs";
export const order = 3;
export const issues: Issue[] = [
  { number: 96, title: "Song Director" },
  { number: 47, title: "Song analysis: grid, sections, energy" },
  { number: 52, title: "VirtualDJ link" },
];

const LIBRARY: Song[] = [...SONGS, ...LIBRARY_ONLY];

const GROUPS: { kind: LookKind; name: string }[] = [
  { kind: "wash", name: "Wash" },
  { kind: "movement", name: "Movement" },
  { kind: "strobe", name: "Strobe" },
  { kind: "pixels", name: "Pixels" },
];

const ANALYSIS: Record<Song["analysis"], string> = {
  analysed: "Analysed",
  queued: "Queued",
  "needs-review": "Needs review",
};

/** A tiny seeded random, so a song's waveform looks the same every time. */
function seeded(seed: string) {
  let h = [...seed].reduce((a, c) => (a * 31 + c.charCodeAt(0)) >>> 0, 7);
  return () => ((h = (h * 1103515245 + 12345) >>> 0) % 1000) / 1000;
}

/** Section start times, in seconds from the start of the song. */
const offsets = (song: Song) => {
  let t = 0;
  return song.sections.map((s) => {
    const start = t;
    t += s.length;
    return { start, end: t };
  });
};
const lengthOf = (song: Song) => song.sections.reduce((a, s) => a + s.length, 0);

export function Screen() {
  const onAir = () => store.currentSong();
  const [songId, setSongId] = createSignal(onAir()?.song.id ?? SONGS[0].id);
  const song = () => LIBRARY.find((s) => s.id === songId())!;
  const analysis = () => store.analysisOf(song());
  const isOnAir = () => onAir()?.song.id === song().id;

  // The selected section: tapped, else the one playing, else the first empty one.
  const [chosen, setChosen] = createSignal<Record<string, number>>({});
  const playingIndex = () => {
    if (!isOnAir()) return -1;
    const pos = store.now() - onAir()!.start;
    return offsets(song()).findIndex((o) => o.start <= pos && pos < o.end);
  };
  const selected = () => {
    const c = chosen()[songId()];
    if (c !== undefined) return c;
    if (playingIndex() >= 0) return playingIndex();
    const empty = song().sections.findIndex((_, i) => !store.sectionLooksOf(song(), i).length);
    return Math.max(0, empty);
  };
  const select = (i: number) => setChosen({ ...chosen(), [songId()]: i });

  // Listening from a section: a local playhead, not the show clock.
  const [listen, setListen] = createSignal<{ songId: string; from: number; at: number } | null>(null);
  const listenPos = () => {
    const l = listen();
    if (!l || l.songId !== songId()) return null;
    const p = l.from + (performance.now() - l.at) / 1000;
    store.now(); // tick with the clock
    return p < lengthOf(song()) ? p : null;
  };
  /** Where the NOW line sits in this song, in seconds, or null. */
  const nowPos = () => listenPos() ?? (isOnAir() ? store.now() - onAir()!.start : null);

  const [message, setMessage] = createSignal("");

  const add = (i: number, lookId: string) => {
    const ids = store.sectionLooksOf(song(), i);
    if (ids.includes(lookId)) {
      setMessage(`${lookById(lookId).name} is already in ${song().sections[i].title}.`);
      return;
    }
    store.setSectionLooks(song(), i, [...ids, lookId], `Add ${lookById(lookId).name}`);
    setMessage(`Added ${lookById(lookId).name} to ${song().sections[i].title}.`);
  };
  const remove = (i: number, lookId: string) =>
    store.setSectionLooks(song(), i, store.sectionLooksOf(song(), i).filter((x) => x !== lookId), `Remove ${lookById(lookId).name}`);

  const fill = () => {
    const n = store.fillFromStyle(song());
    setMessage(n ? `Filled ${n} empty ${n === 1 ? "section" : "sections"} from the ${store.state.autopilot.style} style.` : "No empty sections to fill.");
  };

  const wave = createMemo(() => {
    const s = song();
    const rnd = seeded(s.id);
    const n = 200;
    const len = lengthOf(s);
    const offs = offsets(s);
    let d = "";
    for (let i = 0; i < n; i++) {
      const t = (i / n) * len;
      const e = s.sections[offs.findIndex((o) => o.start <= t && t < o.end)]?.energy ?? 0.3;
      const h = Math.max(2, (0.25 + 0.75 * rnd()) * e * 52);
      d += `M${i + 0.5},${28 - h / 2}v${h}`;
    }
    return d;
  });

  const pct = (t: number) => (t / lengthOf(song())) * 100;
  const notReady = () => analysis() === "queued";
  const emptyCount = () => song().sections.filter((_, i) => !store.sectionLooksOf(song(), i).length).length;

  return (
    <div class="songs-screen">
      <Region id="library" title="Song library" class="songs-lib" aside={<span class="mute">{LIBRARY.length} songs</span>}>
        <List
          label="Song library"
          selected={songId()}
          onSelect={(id) => {
            setSongId(id);
            setMessage("");
          }}
          items={LIBRARY.map((s) => {
            const a = store.analysisOf(s);
            return {
              id: s.id,
              title: (
                <>
                  {s.title}
                  <Show when={onAir()?.song.id === s.id}>
                    {" "}
                    <span class="now-flag songs-flag">On air</span>
                  </Show>
                </>
              ),
              sub: a === "queued" ? `${s.artist} · not analysed yet` : `${s.artist} · ${s.bpm} BPM · ${Math.round(s.confidence * 100)}% sure`,
              meta: <span class={`songs-state is-${a}`}>{ANALYSIS[a]}</span>,
            };
          })}
        />
        <p class="mute songs-sample">Songs are sample data.</p>
      </Region>

      <div class="songs-centre">
        <Region id="song" title="Song" bare class="songs-song">
          <header class="songs-head">
            <div class="songs-title">
              <h2 class="display">{song().title}</h2>
              <span class="mute">
                {song().artist} · {notReady() ? "tempo not known yet" : `${song().bpm} BPM`} · {mmss(lengthOf(song()))}
                <Show when={isOnAir()}> · on air</Show>
              </span>
            </div>
            <div class="songs-actions">
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
                  onClick={() => setListen({ songId: songId(), from: offsets(song())[selected()].start, at: performance.now() })}
                >
                  Play from {song().sections[selected()]?.title ?? "start"}
                </Button>
              </Show>
              <Button
                variant="primary"
                disabled={notReady() || emptyCount() === 0}
                reason={notReady() ? "Not analysed yet" : "Every section has looks"}
                onClick={fill}
              >
                Fill from style
              </Button>
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
                <p class="mute">Its tempo, sections and energy appear here when the analysis is done. Nothing to do now.</p>
              </div>
            }
          >
            <div class="songs-ruler">
              <svg class="songs-wave" viewBox="0 0 200 56" preserveAspectRatio="none" role="img" aria-label={`Waveform of ${song().title}`}>
                <path d={wave()} vector-effect="non-scaling-stroke" />
              </svg>
              <div class="songs-sections" role="group" aria-label="Sections">
                <For each={song().sections}>
                  {(sec, i) => {
                    const o = () => offsets(song())[i()];
                    return (
                      <button
                        type="button"
                        class="songs-sec"
                        classList={{ "is-selected": selected() === i(), "is-empty": !store.sectionLooksOf(song(), i()).length }}
                        style={{ left: `${pct(o().start)}%`, width: `calc(${pct(o().end) - pct(o().start)}% - 3px)` }}
                        aria-pressed={selected() === i()}
                        onClick={() => select(i())}
                      >
                        <b class="display">{sec.title}</b>
                        <span>{mmss(o().start)}</span>
                      </button>
                    );
                  }}
                </For>
              </div>
              <Show when={nowPos() !== null}>
                <span class="songs-now" style={{ left: `${pct(nowPos()!)}%` }} aria-hidden="true" />
                <span class="now-flag songs-now-flag" style={{ left: `clamp(30px, ${pct(nowPos()!)}%, calc(100% - 50px))` }}>
                  {listenPos() !== null ? "Listening" : "Now"} {mmss(nowPos()!)}
                </span>
              </Show>
            </div>

            <div class="songs-blocks" role="list" aria-label="Looks per section">
              <For each={song().sections}>
                {(sec, i) => {
                  const ids = () => store.sectionLooksOf(song(), i());
                  return (
                    <div
                      role="listitem"
                      class="songs-block"
                      classList={{ "is-selected": selected() === i(), "is-playing": nowPos() !== null && i() === offsets(song()).findIndex((o) => o.start <= nowPos()! && nowPos()! < o.end) }}
                      onClick={() => select(i())}
                      onDragOver={(e) => e.preventDefault()}
                      onDrop={(e) => {
                        e.preventDefault();
                        const id = e.dataTransfer?.getData("text/plain");
                        if (id) {
                          select(i());
                          add(i(), id);
                        }
                      }}
                    >
                      <header>
                        <b class="display">{sec.title}</b>
                        <span class="mute">{Math.round(sec.energy * 10)}/10</span>
                      </header>
                      <ul>
                        <For each={ids()} fallback={<li class="songs-empty">Empty. Pick this section, then tap a look on the right.</li>}>
                          {(lookId) => (
                            <li class="songs-look">
                              <i style={{ background: lookById(lookId).colors[0] }} />
                              <span>{lookById(lookId).name}</span>
                              <button
                                type="button"
                                class="songs-x"
                                aria-label={`Remove ${lookById(lookId).name} from ${sec.title}`}
                                onClick={(e) => {
                                  e.stopPropagation();
                                  remove(i(), lookId);
                                }}
                              >
                                <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
                                  <path d="M2 2l8 8M10 2l-8 8" stroke="currentColor" stroke-width="2.5" />
                                </svg>
                              </button>
                            </li>
                          )}
                        </For>
                      </ul>
                    </div>
                  );
                }}
              </For>
            </div>
            <p class="songs-msg" role="status">
              {message()}
            </p>
          </Show>
        </Region>

        <Region id="energy" title="Energy" class="songs-energy" aside={<span class="mute">How hard the song hits, section by section</span>}>
          <Show when={!notReady()} fallback={<p class="mute">Appears after the analysis.</p>}>
            <EnergyCurve
              label={`Energy of ${song().title}`}
              from={0}
              to={lengthOf(song())}
              now={nowPos() ?? -1}
              spans={song().sections.map((s, i) => ({ ...offsets(song())[i], energy: s.energy, title: s.title }))}
            />
          </Show>
        </Region>
      </div>

      <Region
        id="looks"
        title="Looks to add"
        class="songs-looks"
        aside={<span class="mute">to {notReady() ? "nothing yet" : song().sections[selected()]?.title}</span>}
      >
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
                          draggable={!notReady()}
                          disabled={notReady()}
                          onDragStart={(e) => e.dataTransfer?.setData("text/plain", l.id)}
                          onClick={() => add(selected(), l.id)}
                          aria-label={`Add ${l.name} to ${song().sections[selected()]?.title}`}
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
