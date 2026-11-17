import { createMemo, createSignal, For, Show } from "solid-js";
import { store, type Fixture } from "../../fake/store";
import { Button, Fader, List, Region, Table, Tabs } from "../../kit";
import type { Issue } from "../../shell/registry";
import "./dmx.css";

export const id = "dmx";
export const title = "DMX";
export const order = 6;
export const issues: Issue[] = [
  { number: 48, title: "DMX monitor and channel tester" },
  { number: 30, title: "DMX engine: universes, merge, masters" },
];

interface Group {
  key: string;
  title: string;
  fixture: Fixture | null;
  channels: { address: number; name: string }[];
}

const STATE: Record<string, string> = { connected: "Sending", reconnecting: "Reconnecting", error: "Error" };

export function Screen() {
  const [universe, setUniverse] = createSignal(1);
  const [patchedOnly, setPatchedOnly] = createSignal(false);
  const [changingOnly, setChangingOnly] = createSignal(false);
  const [selected, setSelected] = createSignal<number | null>(66);

  const values = createMemo(() => store.channelValues(universe()));
  /** Channels whose value moves with the beat. */
  const changing = createMemo(() => {
    const a = store.channelValues(universe(), 0.25);
    const b = store.channelValues(universe(), 0.5);
    const v = values();
    return new Set(v.map((x, i) => (i > 0 && (x !== a[i] || x !== b[i]) ? i : 0)).filter(Boolean));
  });

  const fixtures = () => store.rig.filter((f) => f.universe === universe()).sort((a, b) => a.address - b.address);
  const ownerOf = (address: number) => fixtures().filter((f) => address >= f.address && address < f.address + f.channels);
  const nameOf = (address: number) => {
    const f = ownerOf(address)[0];
    return f ? `${f.name}, ${store.attributes(f)[address - f.address]}` : "Not patched";
  };
  const clash = (address: number) => ownerOf(address).length > 1;
  const held = (address: number) => universe() === 1 && store.state.held[address] !== undefined;

  // Fixtures in address order, then the free ranges between them.
  const groups = createMemo((): Group[] => {
    const out: Group[] = fixtures().map((f) => ({
      key: f.id,
      title: f.name,
      fixture: f,
      channels: store.attributes(f).map((name, i) => ({ address: f.address + i, name })),
    }));
    if (!patchedOnly()) {
      const used = new Set(out.flatMap((g) => g.channels.map((c) => c.address)));
      let start = 0;
      for (let a = 1; a <= 513; a++) {
        const free = a <= 512 && !used.has(a);
        if (free && !start) start = a;
        if (!free && start) {
          out.push({
            key: `free-${start}`,
            title: "Not patched",
            fixture: null,
            channels: Array.from({ length: a - start }, (_, i) => ({ address: start + i, name: "" })),
          });
          start = 0;
        }
      }
    }
    return out;
  });
  const shown = (g: Group) =>
    changingOnly() ? g.channels.filter((c) => changing().has(c.address) || held(c.address)) : g.channels;

  const output = () => store.outputs().find((o) => o.universe === universe());
  const heldList = () =>
    Object.entries(store.state.held)
      .map(([a, v]) => ({ address: Number(a), value: v }))
      .sort((a, b) => a.address - b.address);

  return (
    <div class="dmx-screen">
      <Region id="universes" title="Universes" bare class="dmx-top">
        <Tabs
          label="Universes"
          value={String(universe())}
          onChange={(u) => setUniverse(Number(u))}
          items={[1, 2].map((u) => {
            const o = store.outputs().find((x) => x.universe === u);
            return {
              id: String(u),
              label: (
                <>
                  Universe {u}
                  <Show when={o && o.state !== "connected"}>
                    <i class={`dmx-tab-mark is-${o!.state}`} aria-hidden="true" />
                  </Show>
                </>
              ),
              ariaLabel: `Universe ${u}${o && o.state !== "connected" ? `, ${STATE[o.state].toLowerCase()}` : ""}`,
            };
          })}
        />
        <div class="dmx-filters" role="group" aria-label="Show">
          <Button pressed={patchedOnly()} onClick={() => setPatchedOnly(!patchedOnly())}>
            Patched only
          </Button>
          <Button pressed={changingOnly()} onClick={() => setChangingOnly(!changingOnly())}>
            Changing
          </Button>
        </div>
      </Region>

      <Region
        id="channels"
        title="Channels"
        class="dmx-grid-region"
        aside={
          <span class="dmx-legend">
            <span>
              <i class="dmx-key is-held" /> Held by hand
            </span>
            <span>
              <i class="dmx-key is-changing" /> Changing
            </span>
            <span>
              <i class="dmx-key is-clash" /> Clash
            </span>
          </span>
        }
      >
        <Show when={output() && output()!.state !== "connected"}>
          {(_) => (
            <p class={`dmx-banner is-${output()!.state}`} role="status">
              <strong>
                {output()!.device}: {STATE[output()!.state].toLowerCase()}.
              </strong>{" "}
              {output()!.state === "error" ? `${output()!.detail}. Check its cable and power; the values below are not reaching the lights.` : `${output()!.detail}.`}
            </p>
          )}
        </Show>
        <div class="dmx-scroll">
          <Show when={groups().some((g) => shown(g).length)} fallback={<p class="mute dmx-none">{changingOnly() ? "Nothing is changing right now." : `Nothing patched in universe ${universe()}.`}</p>}>
            <For each={groups()}>
              {(g) => (
                <Show when={shown(g).length}>
                  <section class="dmx-group" classList={{ "is-free": !g.fixture }}>
                    <header>
                      <b>{g.title}</b>
                      <span class="mute">
                        {g.channels[0].address} to {g.channels[g.channels.length - 1].address}
                        <Show when={g.fixture}> · {g.fixture!.model}</Show>
                      </span>
                    </header>
                    <div class="dmx-cells">
                      <For each={shown(g)}>
                        {(c) => (
                          <button
                            type="button"
                            class="dmx-cell"
                            classList={{
                              "is-held": held(c.address),
                              "is-selected": selected() === c.address,
                              "is-changing": changing().has(c.address),
                              "is-clash": clash(c.address),
                            }}
                            aria-label={`Channel ${c.address}, ${nameOf(c.address)}, ${values()[c.address]}${held(c.address) ? ", held by hand" : ""}`}
                            onClick={() => setSelected(c.address)}
                          >
                            <span class="dmx-addr">{c.address}</span>
                            <span class="dmx-name">{c.name}</span>
                            <b class="dmx-val">{values()[c.address]}</b>
                            <i class="dmx-bar" style={{ width: `${(values()[c.address] / 255) * 100}%` }} />
                          </button>
                        )}
                      </For>
                    </div>
                  </section>
                </Show>
              )}
            </For>
          </Show>
        </div>
      </Region>

      <div class="dmx-side">
        <Region id="channel" title="Channel" class="dmx-channel">
          <Show when={selected()} fallback={<p class="mute">Tap a channel to get a fader for it.</p>}>
            {(a) => (
              <>
                <p class="dmx-ch-name">
                  <b class="display">{a()}</b>
                  <span>{nameOf(a())}</span>
                </p>
                <Show
                  when={universe() === 1}
                  fallback={<p class="mute">Universe {universe()} has no working output, so its channels cannot be set by hand.</p>}
                >
                  <Fader
                    orientation="horizontal"
                    label={`Channel ${a()}`}
                    layer="manual"
                    value={Math.round((values()[a()] / 255) * 100)}
                    valueText={String(values()[a()])}
                    onChange={(v) => store.holdChannel(a(), (v / 100) * 255)}
                  />
                  <div class="dmx-ch-actions">
                    <Button onClick={() => store.holdChannel(a(), 0)}>Zero</Button>
                    <Button onClick={() => store.holdChannel(a(), 255)}>Full</Button>
                    <Button disabled={!held(a())} reason="Not held" onClick={() => store.releaseChannel(a())}>
                      Release
                    </Button>
                  </div>
                  <p class="mute">Moving the fader holds the channel by hand. Release gives it back to the layers.</p>
                </Show>
              </>
            )}
          </Show>
        </Region>

        <Region id="held" title="Held by hand" class="dmx-held" aside={<span class="mute">{heldList().length} channels</span>}>
          <List
            label="Held by hand"
            selected={selected() ? String(selected()) : null}
            onSelect={(a) => {
              setUniverse(1);
              setSelected(Number(a));
            }}
            empty="Nothing held. Every channel follows the layers."
            items={heldList().map((h) => ({
              id: String(h.address),
              title: `${h.address} · ${nameOf(h.address)}`,
              meta: <b>{h.value}</b>,
              layer: "manual",
              actions: (
                <Button class="dmx-release" aria-label={`Release channel ${h.address}`} onClick={() => store.releaseChannel(h.address)}>
                  Release
                </Button>
              ),
            }))}
          />
          <Button variant="layer" layer="manual" disabled={!heldList().length} reason="Nothing held" onClick={() => store.releaseAllChannels()}>
            Release all
          </Button>
        </Region>
      </div>

      <Region id="output" title="Output" class="dmx-output">
        <Table
          dense
          caption="Output per universe"
          rows={store.outputs().filter((o) => o.universe !== null)}
          rowKey={(o) => o.id}
          onRowClick={(o) => o.universe && o.universe <= 2 && setUniverse(o.universe)}
          selected={output()?.id ?? null}
          columns={[
            { key: "universe", title: "Universe", width: "90px", render: (o) => `Universe ${o.universe}` },
            { key: "device", title: "Device", render: (o) => `${o.device} (${o.kind})` },
            { key: "state", title: "State", width: "130px", render: (o) => <span class={`dmx-state is-${o.state}`}>{STATE[o.state]}</span> },
            { key: "fps", title: "Frames per second", width: "150px", align: "right", render: (o) => String(o.fps) },
            { key: "errors", title: "Errors", width: "80px", align: "right", render: (o) => String(o.errors) },
            { key: "detail", title: "", render: (o) => <span class="mute">{o.detail}</span> },
          ]}
        />
      </Region>
    </div>
  );
}
