// Autopilot: all its settings live here. A style is a named set of rules;
// a rule says when (song section), pick (one look from a group) and fit.
// The Console and the LIVE switch keep only on or off, and which style.
import { createSignal, For, Show } from "solid-js";
import { FIT_NAME, SECTION_NAME, store, type Fit } from "../../fake/store";
import type { SectionLabel } from "../../fake/data";
import { Button, Fader, Region } from "../../kit";

export function Autopilot(props: { styleId?: string; ruleId?: string }) {
  const ap = () => store.state.autopilot;
  const [styleId, setStyleId] = createSignal(props.styleId ?? ap().style);
  const style = () => ap().styles.find((s) => s.id === styleId()) ?? ap().styles[0];
  const [marked, setMarked] = createSignal(props.ruleId ?? null);
  const countIn = (group: string) => store.items().filter((i) => i.groups.includes(group)).length;

  return (
    <div class="ap">
      <Region id="styles" title="Styles" class="ap-styles" aside={<span class="mute">A named set of rules</span>}>
        <ul class="ap-style-list">
          <For each={ap().styles}>
            {(s) => (
              <li>
                <button type="button" class="ap-style" aria-current={style().id === s.id ? "true" : undefined} onClick={() => (setStyleId(s.id), setMarked(null))}>
                  <b>{s.name}</b>
                  <span class="mute">{s.rules.length} rules</span>
                  <Show when={ap().style === s.id}>
                    <i class="ap-inuse">In use</i>
                  </Show>
                </button>
              </li>
            )}
          </For>
        </ul>
        <Button onClick={() => setStyleId(store.addStyle())}>New style</Button>
        <p class="mute">On or off, and which style, is chosen on the Console and by the LIVE switch. Everything else is set here.</p>
      </Region>

      <Region id="rules" title="Rules" class="ap-rules" aside={<span class="mute">When, pick, fit</span>}>
        <header class="ap-style-head">
          <label>
            <span class="mute">Style name</span>
            <input type="text" value={style().name} onChange={(e) => store.renameStyle(style().id, e.currentTarget.value.trim() || style().name)} />
          </label>
          <Button variant="layer" layer="autopilot" pressed={ap().style === style().id} onClick={() => store.setAutopilot("style", style().id)}>
            {ap().style === style().id ? "In use" : "Use this style"}
          </Button>
          <Button disabled={ap().styles.length < 2} reason="The last style stays" onClick={() => (store.deleteStyle(style().id), setStyleId(ap().styles[0].id))}>
            Delete style
          </Button>
        </header>
        <ol class="ap-rule-list">
          <li class="ap-rule ap-rule-head" aria-hidden="true">
            <span>When</span>
            <span />
            <span>Pick one from</span>
            <span />
            <span>Fit</span>
            <span />
          </li>
          <For each={style().rules} fallback={<li class="mute ap-none">No rules yet: with this style the autopilot plays only "Never dark".</li>}>
            {(r) => (
              <li class="ap-rule" classList={{ "is-marked": marked() === r.id }}>
                <select aria-label="When: song section" value={r.when} onChange={(e) => store.setStyleRule(style().id, r.id, { when: e.currentTarget.value as SectionLabel })}>
                  <For each={Object.entries(SECTION_NAME)}>{([v, n]) => <option value={v}>{n}</option>}</For>
                </select>
                <Arrow />
                <span class="ap-pick">
                  <select aria-label="Pick one look from this group" value={r.pick} onChange={(e) => store.setStyleRule(style().id, r.id, { pick: e.currentTarget.value })}>
                    <For each={store.allGroups()}>{(g) => <option value={g}>{g}</option>}</For>
                  </select>
                  <small classList={{ "is-empty": !countIn(r.pick) }}>{countIn(r.pick) ? `${countIn(r.pick)} looks` : "empty group"}</small>
                </span>
                <Arrow />
                <select aria-label="Fit: how it lines up with the music" value={r.fit} onChange={(e) => store.setStyleRule(style().id, r.id, { fit: e.currentTarget.value as Fit })}>
                  <For each={Object.entries(FIT_NAME)}>{([v, n]) => <option value={v}>{n}</option>}</For>
                </select>
                <button type="button" class="ap-remove" aria-label={`Remove the rule for ${SECTION_NAME[r.when]}, ${r.pick}`} onClick={() => store.removeRule(style().id, r.id)}>
                  <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
                    <path d="M2 2l8 8M10 2l-8 8" stroke="currentColor" stroke-width="2.5" />
                  </svg>
                </button>
              </li>
            )}
          </For>
        </ol>
        <Button class="ap-add" onClick={() => setMarked(store.addRule(style().id))}>
          Add a rule
        </Button>
        <p class="mute">Several rules for the same section play together. A section with no rule gets "Never dark".</p>
      </Region>

      <Region id="limits" title="For every style" class="ap-limits">
        <section>
          <h3 class="lk-label">Lights it may use</h3>
          <div class="ap-groups" role="group" aria-label="Lights the autopilot may use">
            <For each={store.groups}>
              {(g) => (
                <Button pressed={ap().groups[g.id]} onClick={() => store.toggleGroup(g.id)}>
                  {g.name}
                </Button>
              )}
            </For>
          </div>
        </section>
        <section>
          <h3 class="lk-label">How wild</h3>
          <Fader orientation="horizontal" bare layer="autopilot" label="How wild" value={ap().wild} valueText={ap().wild < 34 ? "calm" : ap().wild < 67 ? "lively" : "wild"} onChange={(v) => store.setAutopilot("wild", v)} />
          <p class="mute">How often it changes its pick inside a section.</p>
        </section>
        <section>
          <h3 class="lk-label">Never dark</h3>
          <select aria-label="Never dark: what plays when nothing else does" value={ap().neverDark ?? ""} onChange={(e) => store.setNeverDark(e.currentTarget.value || null)}>
            <option value="">Nothing: the room may go dark</option>
            <For each={[...store.items()].sort((a, b) => a.name.localeCompare(b.name))}>{(i) => <option value={i.id}>{i.name}</option>}</For>
          </select>
          <p class="mute">What plays when nothing else does: no rule fits, the show is silent and you hold nothing.</p>
        </section>
        <section class="ap-now">
          <h3 class="lk-label">Right now</h3>
          <p>
            <b>{ap().on ? (store.autopilotArmed() ? "On" : "On, waits for LIVE") : "Off"}</b>, style <b>{ap().styles.find((s) => s.id === ap().style)?.name}</b>
          </p>
          <p class="mute">
            {store.autopilotNow().length ? `Playing ${store.autopilotNow().map((l) => l.name).join(", ")}` : "Playing nothing"}
          </p>
        </section>
      </Region>
    </div>
  );
}

function Arrow() {
  return (
    <svg class="ap-arrow" width="22" height="12" viewBox="0 0 22 12" aria-hidden="true">
      <path d="M0 6h18M13 1l6 5-6 5" fill="none" stroke="currentColor" stroke-width="2" />
    </svg>
  );
}
