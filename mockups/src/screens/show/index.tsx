import { createMemo, createSignal, For, Show } from "solid-js";
import { hhmm, lookById, mmss, store, type PlacedSection, type SetEntry } from "../../fake/store";
import { Button, Decks, List, Region, Segmented } from "../../kit";
import type { Issue } from "../../shell/registry";
import { Timeline, type Mode } from "./Timeline";
import "./show.css";

export const id = "show";
export const title = "Show";
export const order = 2;
export const nowStrip = false;
export const issues: Issue[] = [
  { number: 85, title: "Show timeline with waveform" },
  { number: 94, title: "Record what you busk" },
  { number: 95, title: "Follow timecode" },
  { number: 99, title: "DJ set shows" },
  { number: 70, title: "Cue lists" },
  { number: 87, title: "Autopilot: the show follows the music" },
  { number: 86, title: "Never-dark fallback" },
  { number: 68, title: "Looks generated from your rig" },
];

const MARK: Record<SetEntry["mark"], { text: string; cls: string }> = {
  recorded: { text: "Recorded", cls: "is-show" },
  show: { text: "Show", cls: "is-show" },
  auto: { text: "Auto", cls: "is-auto" },
  "not analysed": { text: "Not analysed", cls: "is-none" },
};

/** What plays in a section: the show's clips if it has them, else the autopilot's pick. */
function sectionPlays(p: PlacedSection) {
  const clips = store.state.clips.filter((c) => c.start < p.end && c.end > p.start);
  if (clips.length) return { by: "Show", looks: [...new Set(clips.map((c) => c.lookId))].map(lookById) };
  return { by: "Auto", looks: store.autopilotLooks(p) };
}

export function Screen() {
  const [mode, setMode] = createSignal<Mode>("edit");
  const [playing, setPlaying] = createSignal(true);
  const [follows, setFollows] = createSignal<"internal" | "deck-a" | "timecode">("deck-a");
  const [chosenSong, setChosenSong] = createSignal<number | null>(null);
  const [selectedClip, setSelectedClip] = createSignal<number | null>(null);
  const [chosenPick, setChosenPick] = createSignal<string | null>(null);
  const [cueShift, setCueShift] = createSignal(0);

  // The set only changes when a song ends or a mark changes, not on every clock tick.
  const set = createMemo(() => store.setList(), undefined, {
    equals: (a, b) => a.length === b.length && a.every((e, i) => e.start === b[i].start && e.mark === b[i].mark),
  });
  const currentIndex = createMemo(() => set().findIndex((e) => e.start <= store.now() && store.now() < e.end));
  const entry = createMemo(() => set()[chosenSong() ?? Math.max(0, currentIndex())]);
  const sections = () => store.sectionsBetween(entry().start, entry().end - 0.001);

  /** The selected autopilot pick: the one you tapped, else the next one that is not past. */
  const pick = createMemo(() => {
    const open = sections().filter((p) => !store.covered(p));
    return open.find((p) => p.key === chosenPick()) ?? open.find((p) => p.end > store.now()) ?? null;
  });

  // Run mode: Now / Next over the sections of the song, Go and Back move the cue.
  const nowIndex = () => {
    const i = sections().findIndex((p) => p.start <= store.now() && store.now() < p.end);
    return Math.max(0, Math.min(sections().length - 1, (i < 0 ? 0 : i) + cueShift()));
  };

  return (
    <div class="show-screen" classList={{ "is-run": mode() === "run", "is-record": mode() === "record" }}>
      <div class="show-top">
        <Region id="decks" title="Decks" bare class="show-decks">
          <Decks decks={store.decks()} />
        </Region>
        <Region id="mode" title="Mode" bare class="show-mode">
          <Segmented
            label="Mode"
            size="lg"
            value={mode()}
            onChange={(m) => {
              setMode(m);
              setCueShift(0);
            }}
            options={[
              { value: "edit", label: "Edit" },
              { value: "record", label: "Record" },
              { value: "run", label: "Run" },
            ]}
          />
        </Region>
      </div>

      <div class="show-body">
        <Region id="set" title="Tonight's set" class="show-set">
          <List
            label="Tonight's set"
            selected={entry() ? String(entry().index) : null}
            onSelect={(i) => {
              setChosenSong(Number(i));
              setChosenPick(null);
              setSelectedClip(null);
            }}
            items={set().map((e) => ({
              id: String(e.index),
              title: (
                <>
                  {e.index + 1}. {e.song.title}
                  <Show when={e.index === currentIndex()}>
                    {" "}
                    <span class="now-flag show-set-flag">Now</span>
                  </Show>
                </>
              ),
              sub: `${e.song.artist}, ${hhmm(e.start)}`,
              meta: <span class={`show-mark ${MARK[e.mark].cls}`}>{MARK[e.mark].text}</span>,
            }))}
          />
          <p class="mute show-sample">Songs are sample data.</p>
        </Region>

        <Region
          id="timeline"
          title="Timeline"
          class="show-tl"
          aside={
            <span class="show-transport">
              <Button pressed={playing()} onClick={() => setPlaying(!playing())} aria-label={playing() ? "Stop" : "Play"}>
                <Show
                  when={playing()}
                  fallback={
                    <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
                      <path d="M3 1.5l9 5.5-9 5.5z" fill="currentColor" />
                    </svg>
                  }
                >
                  <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
                    <rect x="2" y="2" width="10" height="10" fill="currentColor" />
                  </svg>
                </Show>
                {playing() ? "Stop" : "Play"}
              </Button>
              <Button
                variant="danger"
                pressed={store.isRecording()}
                onClick={() => store.toggleIndicator("record")}
                aria-label={store.isRecording() ? "Recording. Turn recording off" : "Record"}
              >
                <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
                  <circle cx="7" cy="7" r="5.5" fill="currentColor" />
                </svg>
                {store.isRecording() ? "Recording" : store.state.night.indicators.record ? "Rec when LIVE" : "Record"}
              </Button>
              <Segmented
                label="Follows"
                value={follows()}
                onChange={setFollows}
                options={[
                  { value: "internal", label: "Internal clock" },
                  { value: "deck-a", label: "VirtualDJ deck A" },
                  { value: "timecode", label: "Timecode" },
                ]}
              />
            </span>
          }
        >
          <Show when={mode() === "record"}>
            <p class="show-record-note" role="status">
              {store.isRecording()
                ? "Recording: every press on the Console lands on the Recorded track."
                : "Not recording yet: turn LIVE on, or turn Rec on beside it."}
            </p>
          </Show>
          <Show when={entry()} keyed>
            {(e) => (
              <Timeline
                song={e.song}
                from={e.start}
                to={e.end}
                mode={mode()}
                selectedClip={selectedClip()}
                onSelectClip={setSelectedClip}
                selectedPick={pick()?.key ?? null}
                onSelectPick={setChosenPick}
              />
            )}
          </Show>
          <div class="show-pick layer-autopilot" role="group" aria-label="Selected autopilot pick">
            <Show
              when={pick()}
              fallback={<span class="mute">Nothing left for the autopilot in this song. The show covers it.</span>}
            >
              {(p) => (
                <>
                  <span class="show-pick-text">
                    <b class="display">{p().section.title}</b>
                    <span class="mute">
                      {hhmm(p().start)}, {store.pickReason(p())}: {store.autopilotLooks(p()).map((l) => l.name).join(", ") || "nothing fits the groups"}
                    </span>
                  </span>
                  <span class="show-pick-actions">
                    <Button disabled={p().end <= store.now()} onClick={() => store.skipPick(p())}>
                      Skip
                    </Button>
                    <Button disabled={p().end <= store.now()} onClick={() => store.morePick(p())}>
                      More like this
                    </Button>
                    <Button variant="primary" disabled={p().end <= store.now()} onClick={() => store.keepPick(p())}>
                      Keep
                    </Button>
                  </span>
                </>
              )}
            </Show>
          </div>
        </Region>

        <Show when={mode() === "run"}>
          <Region id="now-next" title="Now and next" class="show-run">
            <ol class="show-cues">
              <For each={sections().slice(nowIndex(), nowIndex() + 4)}>
                {(p, i) => {
                  const plays = () => sectionPlays(p);
                  return (
                    <li classList={{ "is-now": i() === 0 }}>
                      <span class="show-cue-head">
                        <Show when={i() === 0} fallback={<span class="show-cue-when mute">{i() === 1 ? "Next" : hhmm(p.start)}</span>}>
                          <span class="now-flag">Now</span>
                        </Show>
                        <b class="display">{p.section.title}</b>
                        <span class={`show-mark ${plays().by === "Show" ? "is-show" : "is-auto"}`}>{plays().by}</span>
                      </span>
                      <span class="mute">
                        {plays().looks.map((l) => l.name).join(", ")}
                        <Show when={i() === 0}> · {mmss(p.end - store.now())} left</Show>
                      </span>
                    </li>
                  );
                }}
              </For>
            </ol>
            <div class="show-cue-actions">
              <Button size="lg" disabled={nowIndex() === 0} onClick={() => setCueShift(cueShift() - 1)}>
                Back
              </Button>
              <Button
                size="lg"
                variant="primary"
                disabled={nowIndex() >= sections().length - 1}
                onClick={() => setCueShift(cueShift() + 1)}
              >
                Go
              </Button>
            </div>
          </Region>
        </Show>
      </div>
    </div>
  );
}
