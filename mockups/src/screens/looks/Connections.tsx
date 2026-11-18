// Connections: what the open look is made of, who uses it, and which lights
// it sets. Every node is a jump.
import { For, Show } from "solid-js";
import { lookById, SECTION_NAME, store } from "../../fake/store";
import { Region } from "../../kit";
import { KindSymbol } from "./Symbol";

export function Connections(props: { id: string; onOpen: (id: string) => void; onRule: (styleId: string, ruleId: string) => void }) {
  const item = () => store.item(props.id)!;
  const madeOf = () => store.madeOf(props.id);
  const pageName = (page: string) => page[0].toUpperCase() + page.slice(1);
  const usedBy = () => [
    ...store.usedIn(props.id).map((s) => ({ key: `seq-${s.id}`, kind: "Sequence", text: s.name, go: () => props.onOpen(s.id) })),
    ...store.padsOf(props.id).map((p) => ({
      key: `pad-${p.page}-${p.pad}`,
      kind: "Console",
      text: `${pageName(p.page)}, pad ${p.pad}`,
      go: () => store.jumpTo("console", { consolePage: p.page }),
    })),
    ...store.sectionsUsing(props.id).map((u) => ({
      key: `song-${u.song.id}-${u.index}`,
      kind: "Song",
      text: `${u.song.title}, ${u.title}`,
      go: () => store.jumpTo("songs", { song: u.song.id, section: u.index }),
    })),
    ...store.rulesUsing(props.id).map((u) => ({
      key: `rule-${u.rule.id}`,
      kind: "Autopilot",
      text: `${u.style.name}, ${SECTION_NAME[u.rule.when]}`,
      go: () => props.onRule(u.style.id, u.rule.id),
    })),
  ];
  /** Which lights it sets, and what on them: a sequence sets what its parts set. */
  const lights = () => {
    const sets = new Map<string, Set<string>>();
    const seen = new Set<string>();
    const collect = (id: string) => {
      const it = store.item(id);
      if (!it || seen.has(id)) return;
      seen.add(id);
      if (it.kind === "sequence") return it.entries.forEach((e) => collect(e.ref));
      for (const g of store.fixturesOf(id)) for (const p of store.propsOf(lookById(id))) sets.set(g, (sets.get(g) ?? new Set()).add(p));
    };
    collect(props.id);
    return store.groups.filter((g) => sets.has(g.id)).map((g) => ({ g: g.id, name: g.name, sets: [...sets.get(g.id)!].join(", ") }));
  };

  return (
    <Region id="connections" title="Connections" class="lk-conn" aside={<span class="mute">Every box is a jump</span>}>
      <div class="lk-conn-map">
        <div class="lk-conn-col is-made">
          <h3 class="lk-label">Made of</h3>
          <For each={madeOf()} fallback={<p class="lk-conn-none">{item().kind === "step" ? "Nothing else: a step is the smallest part" : "Nothing else"}</p>}>
            {(m) => (
              <button type="button" class="lk-node" title={m.name} onClick={() => props.onOpen(m.id)}>
                <KindSymbol kind={m.kind} />
                <span>{m.name}</span>
              </button>
            )}
          </For>
        </div>
        <div class="lk-conn-mid" aria-hidden="true">
          <b>{item().name}</b>
        </div>
        <div class="lk-conn-col is-used">
          <h3 class="lk-label">Used by</h3>
          <For each={usedBy()} fallback={<p class="lk-conn-none">Nobody yet</p>}>
            {(u) => (
              <button type="button" class="lk-node" title={`${u.kind}: ${u.text}`} onClick={u.go}>
                <small>{u.kind}</small>
                <span>{u.text}</span>
              </button>
            )}
          </For>
        </div>
      </div>
      <div class="lk-conn-lights">
        <h3 class="lk-label">Lights</h3>
        <For each={lights()} fallback={<p class="lk-conn-none">No lights yet</p>}>
          {(l) => (
            <button type="button" class="lk-node is-light" onClick={() => store.jumpTo("patch", { fixtureGroup: l.g })}>
              <span>
                <b>{l.name}</b>: {l.sets}
              </span>
            </button>
          )}
        </For>
      </div>
      <Show when={usedBy().length > 1 || store.usedIn(props.id).length}>
        <p class="mute lk-conn-warn">A change here changes it everywhere it is used.</p>
      </Show>
    </Region>
  );
}
