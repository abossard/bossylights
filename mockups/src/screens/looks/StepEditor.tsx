import { For, Show, type JSX } from "solid-js";
import { store, type Step } from "../../fake/store";
import { Button, Fader, XYPad } from "../../kit";
import { GoboPicture } from "./Symbol";
import { OtherColour } from "./Manage";

const PALETTE = ["#ff2a2a", "#ffa000", "#f2b705", "#b6ff2e", "#3ddc97", "#12c2a8", "#8fe8ff", "#2a4bff", "#7b2cff", "#ff2fd0", "#ffb46b", "#ffffff"];

function Card(props: { title: string; set: boolean; onClear: () => void; onSet: () => void; what: string; children: JSX.Element }) {
  return (
    <section class="lk-card" classList={{ "is-unset": !props.set }}>
      <header>
        <h3>{props.title}</h3>
        <Show when={props.set} fallback={<span class="lk-notset">not set</span>}>
          <Button variant="quiet" class="lk-clear" onClick={props.onClear}>
            Clear
          </Button>
        </Show>
      </header>
      <Show
        when={props.set}
        fallback={
          <div class="lk-unset">
            <p class="mute">This step leaves {props.what} alone.</p>
            <Button onClick={props.onSet}>Set {props.what}</Button>
          </div>
        }
      >
        {props.children}
      </Show>
    </section>
  );
}

/** A step: which fixtures, then one card per property. */
export function StepEditor(props: { step: Step }) {
  const s = () => props.step;
  const toggleFixture = (g: Step["fixtures"][number]) => {
    const f = s().fixtures;
    store.setStep(s().id, "fixtures", f.includes(g) ? f.filter((x) => x !== g) : [...f, g]);
  };
  return (
    <div class="lk-step">
      <div class="lk-fixtures" role="group" aria-label="Which fixtures">
        <span class="lk-label">Which fixtures</span>
        <For each={store.groups}>
          {(g) => (
            <Button pressed={s().fixtures.includes(g.id)} onClick={() => toggleFixture(g.id)}>
              {g.name}
            </Button>
          )}
        </For>
      </div>
      <div class="lk-cards">
        <Card title="Colour" what="colour" set={!!s().colour} onClear={() => store.setStep(s().id, "colour", undefined)} onSet={() => store.setStep(s().id, "colour", ["#ffffff"])}>
          <div class="lk-chips">
            <For each={s().colour ?? []}>
              {(c, i) => (
                <button
                  type="button"
                  class="lk-chip"
                  style={{ background: c }}
                  aria-label={`Remove colour ${i() + 1}`}
                  onClick={() => store.setStep(s().id, "colour", (s().colour ?? []).filter((_, j) => j !== i()))}
                >
                  <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
                    <path d="M2 2l8 8M10 2l-8 8" stroke="currentColor" stroke-width="2.5" />
                  </svg>
                </button>
              )}
            </For>
          </div>
          <div class="lk-palette" role="group" aria-label="Add a colour">
            <For each={PALETTE}>
              {(c) => (
                <button
                  type="button"
                  class="lk-swatch"
                  style={{ background: c }}
                  aria-label={`Add ${c}`}
                  disabled={(s().colour ?? []).length >= 4}
                  onClick={() => store.setStep(s().id, "colour", [...(s().colour ?? []), c])}
                />
              )}
            </For>
          </div>
          <OtherColour full={(s().colour ?? []).length >= 4} onAdd={(c) => store.setStep(s().id, "colour", [...(s().colour ?? []), c])} />
        </Card>

        <Card title="Position" what="position" set={!!s().position} onClear={() => store.setStep(s().id, "position", undefined)} onSet={() => store.setStep(s().id, "position", { x: 50, y: 50 })}>
          <XYPad label="Position" x={s().position?.x ?? 50} y={s().position?.y ?? 50} onChange={(x, y) => store.setStep(s().id, "position", { x, y })} class="lk-xy" />
          <span class="mute">
            Pan {s().position?.x}, tilt {s().position?.y}
          </span>
        </Card>

        <Card title="Gobo" what="gobo" set={!!s().gobo} onClear={() => store.setStep(s().id, "gobo", undefined)} onSet={() => store.setStep(s().id, "gobo", "leaf")}>
          <div class="lk-gobos" role="radiogroup" aria-label="Gobo">
            <For each={store.gobos}>
              {(g) => (
                <button type="button" role="radio" class="lk-gobo" aria-checked={s().gobo === g} aria-label={g} onClick={() => store.setStep(s().id, "gobo", g)}>
                  <GoboPicture gobo={g} />
                </button>
              )}
            </For>
          </div>
        </Card>

        <Card title="Dimmer" what="dimmer" set={s().dimmer !== undefined} onClear={() => store.setStep(s().id, "dimmer", undefined)} onSet={() => store.setStep(s().id, "dimmer", 100)}>
          <Fader orientation="horizontal" bare label="Dimmer" value={s().dimmer ?? 0} onChange={(v) => store.setStep(s().id, "dimmer", v)} />
          <span class="lk-value">{s().dimmer}%</span>
        </Card>

        <Card title="Spin" what="spin" set={s().spin !== undefined} onClear={() => store.setStep(s().id, "spin", undefined)} onSet={() => store.setStep(s().id, "spin", 30)}>
          <Fader orientation="horizontal" bare label="Spin" value={s().spin ?? 0} onChange={(v) => store.setStep(s().id, "spin", v)} />
          <span class="lk-value">{(s().spin ?? 0) < 34 ? "slow" : (s().spin ?? 0) < 67 ? "medium" : "fast"}</span>
        </Card>
      </div>
    </div>
  );
}
