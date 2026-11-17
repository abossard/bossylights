import { For } from "solid-js";
import { store, type Pattern, type Shape } from "../../fake/store";
import { Button, Segmented } from "../../kit";
import { ShapePicture } from "./Symbol";

const SHAPES: { value: Shape; label: string }[] = [
  { value: "plasma", label: "Plasma" },
  { value: "stripes", label: "Stripes" },
  { value: "wave", label: "Wave" },
  { value: "sparkle", label: "Sparkle" },
];

/** A pattern: its shape, its measure, where the colours come from, where it runs. */
export function PatternEditor(props: { pattern: Pattern }) {
  const p = () => props.pattern;
  const colourSteps = () => store.items().filter((i) => i.kind === "step" && (i.colour?.length ?? 0) > 0);
  return (
    <div class="lk-pattern">
      <section class="lk-field">
        <span class="lk-label">Shape</span>
        <div class="lk-shapes" role="radiogroup" aria-label="Shape">
          <For each={SHAPES}>
            {(s) => (
              <button type="button" role="radio" class="lk-shape" aria-checked={p().shape === s.value} onClick={() => store.setPattern(p().id, { shape: s.value })}>
                <ShapePicture shape={s.value} />
                {s.label}
              </button>
            )}
          </For>
        </div>
      </section>
      <section class="lk-field">
        <span class="lk-label">Measure</span>
        <div class="lk-stepper">
          <Button aria-label="Shorter" disabled={p().measure <= 1} onClick={() => store.setPattern(p().id, { measure: p().measure / 2 >= 1 ? p().measure / 2 : 1 })}>
            ÷2
          </Button>
          <b class="display">{p().measure}</b>
          <span class="mute">beats per loop</span>
          <Button aria-label="Longer" disabled={p().measure >= 32} onClick={() => store.setPattern(p().id, { measure: p().measure * 2 })}>
            ×2
          </Button>
        </div>
      </section>
      <section class="lk-field">
        <label class="lk-label" for="lk-colours">
          Colours from
        </label>
        <div class="lk-colours-from">
          <select
            id="lk-colours"
            value={p().colours ?? ""}
            onChange={(e) => store.setPattern(p().id, { colours: e.currentTarget.value || null })}
          >
            <option value="">Its own colours</option>
            <For each={colourSteps()}>{(s) => <option value={s.id}>{s.name}</option>}</For>
          </select>
          <span class="lk-chips is-small">
            <For each={store.colorsOf(p().id)}>{(c) => <i style={{ background: c }} />}</For>
          </span>
        </div>
        <p class="mute">Swap the step and the motion stays.</p>
      </section>
      <section class="lk-field">
        <span class="lk-label">Runs on</span>
        <Segmented
          label="Runs on"
          value={p().on}
          onChange={(v) => store.setPattern(p().id, { on: v })}
          options={store.groups.map((g) => ({ value: g.id, label: g.id === "bars" ? "Bars, as pixels" : g.name }))}
        />
      </section>
    </div>
  );
}
