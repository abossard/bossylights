// "Add lights": a guided flow that runs in place, step by step. Its state
// lives outside the screen, so you can leave and come back to it.
import { createSignal, For, Show } from "solid-js";
import { createStore } from "solid-js/store";
import { store, type Fixture } from "../../fake/store";
import { Button } from "../../kit";
import { LIBRARY, newFixture, type Model } from "./library";

interface Flow {
  open: boolean;
  step: 1 | 2 | 3 | 4;
  query: string;
  modelId: string | null;
  modeIndex: number;
  address: number;
  /** step 4: what the person answered */
  answer: "none" | "nothing";
}

const fresh = (): Flow => ({ open: false, step: 1, query: "", modelId: null, modeIndex: 0, address: 1, answer: "none" });
const [flow, setFlow] = createStore<Flow>(fresh());
export { flow };

export const STEPS = ["Find your light", "Pick its mode", "Set its address", "Is this the one?"];

/** True when a flow was started and left before it was finished. */
export const flowInProgress = () => !flow.open && (flow.modelId !== null || flow.step > 1);

export function openFlow() {
  setFlow("open", true);
}

export const model = () => LIBRARY.find((m) => m.id === flow.modelId) ?? null;
export const mode = () => model()?.modes[flow.modeIndex] ?? null;

/** The light being added, while step 4 flashes it. Not in the patch yet. */
export function ghost(): Fixture | null {
  const m = model();
  const md = mode();
  if (!flow.open || flow.step !== 4 || !m || !md) return null;
  return { ...newFixture(store.rig, m, md, flow.address), id: "ghost" };
}

function Picture(props: { kind: Fixture["kind"] }) {
  return (
    <svg class="add-pic" viewBox="0 0 64 48" aria-hidden="true">
      <Show when={props.kind === "par"}>
        <rect x="14" y="8" width="36" height="32" />
        <circle cx="32" cy="24" r="11" class="add-pic-lens" />
      </Show>
      <Show when={props.kind === "mover"}>
        <path d="M14 44h36M20 44V14M44 44V14" />
        <rect x="20" y="10" width="24" height="22" />
        <circle cx="32" cy="21" r="7" class="add-pic-lens" />
      </Show>
      <Show when={props.kind === "bar"}>
        <rect x="4" y="18" width="56" height="12" />
        <For each={[0, 1, 2, 3, 4, 5, 6, 7]}>{(i) => <rect x={7 + i * 6.6} y="21" width="4" height="6" class="add-pic-lens" />}</For>
      </Show>
      <Show when={props.kind === "laser"}>
        <rect x="10" y="16" width="30" height="20" />
        <path d="M40 26l20-14M40 26l20 0M40 26l20 14" class="add-pic-beam" />
      </Show>
    </svg>
  );
}

/** The DIP switches on the back of the light for an address, 1 to 9 with values 1 to 256. */
function Dip(props: { address: number }) {
  const bits = () => Array.from({ length: 9 }, (_, i) => ((props.address >> i) & 1) === 1);
  return (
    <div class="dip" role="img" aria-label={`Switches ${bits().map((b, i) => (b ? i + 1 : null)).filter(Boolean).join(", ")} on`}>
      <For each={bits()}>
        {(on, i) => (
          <span class="dip-sw" classList={{ "is-on": on }}>
            <i />
            <b>{i() + 1}</b>
          </span>
        )}
      </For>
      <span class="dip-on" aria-hidden="true">
        ON
      </span>
    </div>
  );
}

export function AddFlow(props: { onDone: (f: Fixture) => void }) {
  const [hint, setHint] = createSignal("");
  const results = () => {
    const q = flow.query.trim().toLowerCase();
    return LIBRARY.filter((m) => !q || `${m.name} ${m.type}`.toLowerCase().includes(q));
  };
  const pick = (m: Model) => {
    setFlow({ modelId: m.id, modeIndex: Math.min(1, m.modes.length - 1), step: 2 });
  };
  const toAddress = () => {
    const md = mode();
    if (!md) return;
    setFlow({ address: store.freeAddress(md.channels) ?? 1, step: 3 });
  };
  const clash = () => {
    const md = mode();
    if (!md) return null;
    const last = flow.address + md.channels - 1;
    return store.rig.find((f) => f.universe === 1 && flow.address <= f.address + f.channels - 1 && f.address <= last) ?? null;
  };
  const confirm = () => {
    const m = model();
    const md = mode();
    if (!m || !md) return;
    const f = newFixture(store.rig, m, md, flow.address);
    store.addFixture(f);
    setFlow(fresh());
    props.onDone(f);
  };

  return (
    <section class="add" aria-label="Add lights">
      <header class="add-head">
        <div>
          <h2 class="display">Add lights</h2>
          <p class="mute">
            Step {flow.step} of 4: {STEPS[flow.step - 1]}
          </p>
        </div>
        <Button onClick={() => setFlow("open", false)}>Leave for now</Button>
      </header>
      <ol class="add-steps" aria-label="Steps">
        <For each={STEPS}>
          {(s, i) => (
            <li classList={{ "is-done": i() + 1 < flow.step, "is-now": i() + 1 === flow.step }}>
              <span>{i() + 1}</span>
              {s}
            </li>
          )}
        </For>
      </ol>

      <Show when={hint()}>
        <p class="add-clash" role="status">
          {hint()}
        </p>
      </Show>
      <div class="add-body">
        <Show when={flow.step === 1}>
          <input
            type="text"
            placeholder="Search by name or type, e.g. wash"
            aria-label="Search the fixture library"
            value={flow.query}
            onInput={(e) => setFlow("query", e.currentTarget.value)}
          />
          <ul class="add-models">
            <For each={results()} fallback={<li class="mute">Nothing found. Try "par", "moving head" or "laser".</li>}>
              {(m) => (
                <li>
                  <button type="button" class="add-model" classList={{ "is-picked": flow.modelId === m.id }} onClick={() => pick(m)}>
                    <Picture kind={m.kind} />
                    <b>{m.name}</b>
                    <span class="mute">{m.type}</span>
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>

        <Show when={flow.step === 2 && model()}>
          {(m) => (
            <>
              <p>
                <b>{m().name}</b> <span class="mute">can run in {m().modes.length} modes.</span>
              </p>
              <div class="add-modes" role="radiogroup" aria-label="Mode">
                <For each={m().modes}>
                  {(md, i) => (
                    <button
                      type="button"
                      role="radio"
                      aria-checked={flow.modeIndex === i()}
                      class="add-mode"
                      onClick={() => setFlow("modeIndex", i())}
                    >
                      <b>{md.name}</b>
                      <span>{md.about}</span>
                    </button>
                  )}
                </For>
              </div>
              <p class="mute">Not sure? The menu on the light shows its mode.</p>
            </>
          )}
        </Show>

        <Show when={flow.step === 3 && mode()}>
          {(md) => (
            <>
              <p class="mute">The first free place for {md().channels} channels:</p>
              <div class="add-address">
                <Button aria-label="Address down" onClick={() => setFlow("address", Math.max(1, flow.address - 1))}>
                  <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
                    <path d="M2 7h10" stroke="currentColor" stroke-width="3" />
                  </svg>
                </Button>
                <span class="add-addr-num">
                  <b class="display">{flow.address}</b>
                  <span class="mute">
                    to {flow.address + md().channels - 1}, universe 1
                  </span>
                </span>
                <Button aria-label="Address up" onClick={() => setFlow("address", Math.min(513 - md().channels, flow.address + 1))}>
                  <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
                    <path d="M2 7h10M7 2v10" stroke="currentColor" stroke-width="3" />
                  </svg>
                </Button>
              </div>
              <Show when={clash()}>
                {(c) => (
                  <p class="add-clash" role="alert">
                    Clashes with {c().name} ({c().address} to {c().address + c().channels - 1}).
                  </p>
                )}
              </Show>
              <p class="mute">Set the switches on the back of the light like this:</p>
              <Dip address={flow.address} />
            </>
          )}
        </Show>

        <Show when={flow.step === 4}>
          <p class="add-ask">
            <b>The new light is flashing on the map.</b> Did a light in the room flash too?
          </p>
          <div class="add-answers">
            <Button variant="primary" size="lg" onClick={confirm}>
              Yes, that is this one
            </Button>
            <Button
              onClick={() => {
                setHint("Check the cable, the mode and the switches, then flash it again.");
                setFlow("step", 3);
              }}
            >
              Nothing flashed
            </Button>
          </div>
        </Show>
      </div>

      <footer class="add-foot">
        <Button
          disabled={flow.step === 1}
          onClick={() => {
            setHint("");
            setFlow("step", (flow.step - 1) as Flow["step"]);
          }}
        >
          Back
        </Button>
        <Button variant="quiet" onClick={() => setFlow(fresh())}>
          Start over
        </Button>
        <Show when={flow.step === 2}>
          <Button variant="primary" onClick={toAddress}>
            Next: address
          </Button>
        </Show>
        <Show when={flow.step === 3}>
          <Button variant="primary" disabled={!!clash()} reason="Pick a free address" onClick={() => setFlow("step", 4)}>
            Next: flash it
          </Button>
        </Show>
      </footer>
    </section>
  );
}
