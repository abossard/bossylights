import { For, Show } from "solid-js";
import { hhmm, lookById, mmss, store, type AutopilotStyle, type Lane } from "../../fake/store";
import { Button, EnergyCurve, Fader, List, Region, Segmented, Timetable, stepWindow } from "../../kit";
import type { Issue } from "../../shell/registry";
import "./autopilot.css";

export const id = "autopilot";
export const title = "Autopilot";
export const order = 2;
export const nowStrip = false;
export const issues: Issue[] = [
  { number: 87, title: "Autopilot: the show follows the music" },
  { number: 86, title: "Never-dark fallback" },
  { number: 68, title: "Looks generated from your rig" },
];

const STYLES: { value: AutopilotStyle; label: string }[] = [
  { value: "chill", label: "Chill" },
  { value: "club", label: "Club" },
  { value: "peak", label: "Peak" },
  { value: "no-strobe", label: "No strobe" },
];

const wildWord = (v: number) => (v < 25 ? "Calm" : v < 50 ? "Easy" : v < 75 ? "Lively" : "Wild");

function LookChip(props: { id: string }) {
  const look = () => lookById(props.id);
  return (
    <span class="ap-chip">
      <i style={{ background: look().colors[0] }} />
      {look().name}
    </span>
  );
}

function NowAndNext() {
  const song = () => store.currentSong();
  const cur = () => store.currentSection();
  const next = () => store.nextSection();
  const playing = () => (cur() ? store.autopilotLooks(cur()!) : []);
  const coming = () => (next() ? store.autopilotLooks(next()!) : []);

  const rulerLanes = (): Lane[] => [
    {
      layer: "autopilot",
      title: "Song",
      on: store.state.autopilot.on,
      status: "",
      blocks: store.currentSongSections().map((p) => ({ id: p.key, title: p.section.title, start: p.start, end: p.end })),
    },
  ];

  return (
    <Region
      id="now-next"
      title="Now and next"
      class="ap-nownext"
      aside={
        <Show when={song()}>
          {(s) => (
            <span class="ap-song">
              <b>{s().song.title}</b> <span class="mute">{s().song.artist}, sample data</span>
            </span>
          )}
        </Show>
      }
    >
      <Show when={song()}>
        {(s) => (
          <Timetable
            plain
            from={s().start}
            to={s().end}
            now={store.now()}
            lanes={rulerLanes()}
            tickEvery={5 * 60}
            format={(t) => mmss(t - s().start)}
            laneHeight="52px"
            labelWidth="0px"
            class="ap-ruler"
          />
        )}
      </Show>
      <div class="ap-cards">
        <section class="ap-card ap-now" aria-label="Now">
          <header>
            <span class="now-flag">Now</span>
            <h3 class="display">{cur()?.section.title ?? "Nothing playing"}</h3>
            <Show when={cur()}>
              <span class="mute">{mmss(cur()!.end - store.now())} left</span>
            </Show>
          </header>
          <List
            label="Looks playing now"
            empty={store.state.autopilot.on ? "No looks fit the groups it may use." : "Autopilot is off. Live and Show still run."}
            items={(store.state.autopilot.on ? playing() : []).map((l) => ({
              id: l.id,
              title: l.name,
              swatch: l.colors[0],
              meta: store.state.autopilot.likes.includes(l.id) ? "More like this" : undefined,
              actions: (
                <span class="ap-look-actions">
                  <Button onClick={() => store.skipLook(l.id)} aria-label={`Skip ${l.name}`}>
                    Skip this look
                  </Button>
                  <Button
                    pressed={store.state.autopilot.likes.includes(l.id)}
                    onClick={() => store.moreLikeThis(l.id)}
                    aria-label={`More like ${l.name}`}
                  >
                    More like this
                  </Button>
                </span>
              ),
            }))}
          />
        </section>
        <section class="ap-card ap-next" aria-label="Next">
          <header>
            <h3 class="display">Next: {next()?.section.title ?? "end of set"}</h3>
            <Show when={next()}>
              <span class="mute">comes in {mmss(next()!.start - store.now())}</span>
            </Show>
          </header>
          <div class="ap-chips">
            <For each={store.state.autopilot.on ? coming() : []} fallback={<span class="mute">Nothing planned.</span>}>
              {(l) => <LookChip id={l.id} />}
            </For>
          </div>
        </section>
      </div>
    </Region>
  );
}

export function Screen() {
  const win = () => stepWindow(store.now(), 7 * 60, 25 * 60, 5 * 60);
  const song = () => store.currentSong();

  return (
    <div class="ap">
      <Region
        id="timetable"
        title="Layers"
        aside={
          <span class="mute">
            {hhmm(win().from)} to {hhmm(win().to)}
          </span>
        }
      >
        <Timetable
          from={win().from}
          to={win().to}
          now={store.now()}
          lanes={store.lanes(win().from, win().to)}
          tickEvery={5 * 60}
          format={hhmm}
          laneHeight="var(--ap-lane)"
          labelWidth="var(--ap-label)"
        />
      </Region>

      <div class="ap-grid">
        <div class="ap-col">
          <Region id="power" title="Autopilot" class="ap-power">
            <Segmented
              label="Autopilot on or off"
              layer="autopilot"
              size="lg"
              value={store.state.autopilot.on ? "on" : "off"}
              onChange={(v) => store.setAutopilot("on", v === "on")}
              options={[
                { value: "on", label: "On" },
                { value: "off", label: "Off" },
              ]}
            />
            <Segmented
              label="Style"
              value={store.state.autopilot.style}
              onChange={(v) => store.setAutopilot("style", v)}
              options={STYLES}
            />
            <p class="mute">Never dark: if the music stops, a slow warm wash keeps the room lit.</p>
          </Region>

          <Region id="wild" title="How wild" aside={<span class="ap-wild-word display">{wildWord(store.state.autopilot.wild)}</span>}>
            <Fader
              orientation="horizontal"
              bare
              layer="autopilot"
              label="How wild"
              valueText={wildWord(store.state.autopilot.wild)}
              value={store.state.autopilot.wild}
              onChange={(v) => store.setAutopilot("wild", v)}
            />
            <span class="ap-wild-scale mute">
              <span>Calm</span>
              <span>Wild</span>
            </span>
          </Region>

        </div>

        <div class="ap-col ap-mid">
          <NowAndNext />
          <Region id="energy" title="Energy" aside={<span class="mute">How hard this song hits, section by section</span>} class="ap-energy">
            <Show when={song()}>
              {(s) => (
                <EnergyCurve
                  label={`Energy of ${s().song.title}`}
                  layer="autopilot"
                  from={s().start}
                  to={s().end}
                  now={store.now()}
                  spans={store.currentSongSections().map((p) => ({ start: p.start, end: p.end, energy: p.section.energy, title: p.section.title }))}
                />
              )}
            </Show>
          </Region>
        </div>

        <div class="ap-col ap-side">
          <Region id="hands-off" title="Hands off" class="ap-handsoff">
            <p class="mute">The Live layer covers these. The autopilot leaves them alone until you release them.</p>
            <List
              label="Covered by Live"
              empty="Nothing. The autopilot runs every group it may use."
              items={store.handsOff().map((h) => ({
                id: h.group.id,
                title: h.group.name,
                sub: `${h.look.name} since ${hhmm(h.block.start)}`,
                layer: "live",
              }))}
            />
          </Region>
          <Region id="groups" title="Groups it may use">
            <ul class="ap-groups">
              <For each={store.groups}>
                {(g) => (
                  <li>
                    <label>
                      <input
                        type="checkbox"
                        checked={store.state.autopilot.groups[g.id]}
                        onChange={() => store.toggleGroup(g.id)}
                      />
                      <span>{g.name}</span>
                      <span class="mute">{store.rig.filter((f) => f.group === g.id).length} lights</span>
                    </label>
                  </li>
                )}
              </For>
            </ul>
          </Region>
        </div>
      </div>
    </div>
  );
}
