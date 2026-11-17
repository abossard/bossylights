import { createSignal, For, Show } from "solid-js";
import { store, type Fixture } from "../../fake/store";
import { Button, List, Region, RigView, type RigZone } from "../../kit";
import type { Issue } from "../../shell/registry";
import { AddFlow, flow, flowInProgress, ghost, openFlow, STEPS } from "./AddFlow";
import "./patch.css";

export const id = "patch";
export const title = "Patch";
export const order = 4;
export const issues: Issue[] = [
  { number: 50, title: "Patch: fixtures, modes, addresses" },
  { number: 54, title: "Guided patch" },
  { number: 41, title: "Fixture library: QLC+, OFL, GDTF" },
  { number: 56, title: "Groups and the rig map" },
];

const range = (f: Fixture) => `${f.address} to ${f.address + f.channels - 1}`;

export function Screen() {
  const [selectedId, setSelectedId] = createSignal<string>(store.conflicts()[0]?.b.id ?? store.rig[0].id);
  const selected = () => store.fixtureById(selectedId()) ?? null;
  const clashOf = (id: string) => store.conflicts().find((c) => c.a.id === id || c.b.id === id);
  const other = (id: string) => {
    const c = clashOf(id);
    return c ? (c.a.id === id ? c.b : c.a) : null;
  };

  const fixtures = () => {
    const g = ghost();
    return g ? [...store.rig, g] : store.rig;
  };
  const lookOf = (f: Fixture) => {
    if (f.id === "ghost") return { color: "#ffffff", level: store.beatPhase() < 0.5 ? 1 : 0.1 };
    const s = store.fixtureState(f);
    return { color: s.look?.colors[0] ?? null, level: s.level };
  };
  const zones = (): RigZone[] =>
    store.groups
      .map((g) => ({ label: g.name, fixtureIds: store.rig.filter((f) => f.group === g.id).map((f) => f.id) }))
      // a group spread across the whole stage gets no outline: it would cover everything
      .filter((z) => {
        const xs = store.rig.filter((f) => z.fixtureIds.includes(f.id)).map((f) => f.x);
        return z.fixtureIds.length > 1 && Math.max(...xs) - Math.min(...xs) < 0.8;
      });

  const select = (id: string) => {
    setSelectedId(id);
    if (store.state.identify && store.state.identify !== id) store.setIdentify(null);
  };

  return (
    <div class="patch-screen">
      <Region
        id="fixtures"
        title="Fixtures"
        class="patch-list"
        aside={<span class="mute">{store.rig.length} lights</span>}
      >
        <Show
          when={!flow.open}
          fallback={
            <p class="patch-adding">
              Adding a light, step {flow.step} of 4: {STEPS[flow.step - 1]}
            </p>
          }
        >
          <Button variant="primary" class="patch-add" onClick={openFlow}>
            {flowInProgress() ? `Continue adding (step ${flow.step} of 4)` : "Add lights"}
          </Button>
        </Show>
        <div class="patch-scroll">
          <List
            label="Fixtures"
            selected={selectedId()}
            onSelect={select}
            items={store.rig.map((f) => ({
              id: f.id,
              title: f.name,
              sub: `${f.model} · ${f.mode}`,
              meta: (
                <span class="patch-addr" classList={{ "is-clash": store.inConflict(f.id) }}>
                  <Show when={store.inConflict(f.id)}>
                    <i class="patch-clash-mark" aria-hidden="true" />
                    <span class="visually-hidden">Address clash, </span>
                  </Show>
                  {range(f)}
                </span>
              ),
            }))}
          />
        </div>
      </Region>

      <Region id="map" title="Rig map" class="patch-map" aside={<span class="mute">Drag a light to move it. Outlines are groups.</span>}>
        <div class="patch-stage">
          <RigView
            fixtures={fixtures()}
            lookOf={lookOf}
            zones={zones()}
            selected={[selectedId(), ...(ghost() ? ["ghost"] : [])]}
            onFixture={(f) => f.id !== "ghost" && select(f.id)}
            onMove={(f, x, y) => f.id !== "ghost" && store.moveFixture(f.id, x, y)}
            label="Rig map: drag lights to where they hang"
          />
        </div>
      </Region>

      <Region id="details" title={flow.open ? "Add lights" : "Details"} bare class="patch-side">
        <Show
          when={!flow.open}
          fallback={<AddFlow onDone={(f) => setSelectedId(f.id)} />}
        >
          <Show when={selected()} fallback={<p class="mute">Pick a light in the list or on the map.</p>}>
            {(f) => (
              <div class="patch-details">
                <header>
                  <h2 class="display">{f().name}</h2>
                  <span class="mute">
                    {f().model} · {f().mode}
                  </span>
                </header>
                <dl>
                  <div>
                    <dt>Group</dt>
                    <dd>{store.groups.find((g) => g.id === f().group)?.name}</dd>
                  </div>
                  <div>
                    <dt>Channels</dt>
                    <dd>{f().channels}</dd>
                  </div>
                  <div>
                    <dt>Universe</dt>
                    <dd>{f().universe}</dd>
                  </div>
                </dl>
                <div class="patch-address">
                  <span class="mute">Address</span>
                  <div class="patch-stepper">
                    <Button aria-label="Address down" onClick={() => store.setAddress(f().id, f().address - 1)}>
                      <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
                        <path d="M2 7h10" stroke="currentColor" stroke-width="3" />
                      </svg>
                    </Button>
                    <span class="patch-addr-big">
                      <b class="display">{f().address}</b>
                      <span class="mute">to {f().address + f().channels - 1}</span>
                    </span>
                    <Button aria-label="Address up" onClick={() => store.setAddress(f().id, f().address + 1)}>
                      <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
                        <path d="M2 7h10M7 2v10" stroke="currentColor" stroke-width="3" />
                      </svg>
                    </Button>
                  </div>
                </div>
                <Show when={other(f().id)}>
                  {(o) => (
                    <div class="patch-clash" role="alert">
                      <p>
                        <strong>Clashes with {o().name}</strong> on {Math.max(f().address, o().address)} to{" "}
                        {Math.min(f().address + f().channels, o().address + o().channels) - 1}. Both lights get the same data there.
                      </p>
                      <Button
                        onClick={() => {
                          const a = store.freeAddress(f().channels, f().universe, f().id);
                          if (a) store.setAddress(f().id, a);
                        }}
                      >
                        Move to {store.freeAddress(f().channels, f().universe, f().id) ?? "no free"}
                      </Button>
                    </div>
                  )}
                </Show>
                <Button pressed={store.state.identify === f().id} onClick={() => store.setIdentify(store.state.identify === f().id ? null : f().id)}>
                  {store.state.identify === f().id ? "Stop flashing" : "Flash it, to find it in the room"}
                </Button>
              </div>
            )}
          </Show>
        </Show>
      </Region>

      <Region
        id="addresses"
        title="Addresses"
        class="patch-strip"
        aside={
          <span class="mute">
            Universe 1 · {store.rig.filter((f) => f.universe === 1).reduce((a, f) => a + f.channels, 0)} of 512 used
          </span>
        }
      >
        <div class="strip" role="group" aria-label="The 512 addresses of universe 1">
          <For each={[1, 64, 128, 192, 256, 320, 384, 448, 512]}>
            {(a) => (
              <span class="strip-tick" classList={{ "is-end": a === 512 }} style={{ left: a === 512 ? "100%" : `${((a - 1) / 512) * 100}%` }}>
                {a}
              </span>
            )}
          </For>
          <For each={store.rig.filter((f) => f.universe === 1)}>
            {(f) => (
              <button
                type="button"
                class="strip-range"
                classList={{ "is-selected": selectedId() === f.id, "is-clash": store.inConflict(f.id) }}
                style={{ left: `${((f.address - 1) / 512) * 100}%`, width: `${(f.channels / 512) * 100}%` }}
                aria-label={`${f.name}, ${range(f)}`}
                title={`${f.name}, ${range(f)}`}
                onClick={() => select(f.id)}
              />
            )}
          </For>
          <For each={store.conflicts()}>
            {(c) => (
              <span
                class="strip-clash"
                style={{ left: `${((c.from - 1) / 512) * 100}%`, width: `${((c.to - c.from + 1) / 512) * 100}%` }}
                aria-hidden="true"
              />
            )}
          </For>
        </div>
        <p class="strip-legend">
          <span>
            <i class="strip-key" /> In use
          </span>
          <span>
            <i class="strip-key is-selected" /> {selected()?.name ?? "Selected"}
          </span>
          <Show when={store.conflicts().length}>
            <span>
              <i class="strip-key is-clash" /> {store.conflicts().length === 1 ? "1 clash" : `${store.conflicts().length} clashes`}:{" "}
              {store.conflicts().map((c) => `${c.a.name} and ${c.b.name}`).join(", ")}
            </span>
          </Show>
        </p>
      </Region>
    </div>
  );
}
