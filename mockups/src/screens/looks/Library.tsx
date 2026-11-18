// The library: groups are sections that fold. A look can be in many groups.
// Drag a look onto a group to add it. The library can be widened to a full view.
import { createSignal, For, onCleanup, Show } from "solid-js";
import { store, type Item } from "../../fake/store";
import { Button, Region, Segmented } from "../../kit";
import { KindSymbol } from "./Symbol";
import { NewLook } from "./Manage";

type Kind = "all" | Item["kind"];
const NO_GROUP = "In no group";

export function Library(props: { currentId: string; onOpen: (id: string) => void; wide: boolean; onWide: (on: boolean) => void }) {
  const [query, setQuery] = createSignal("");
  const [kind, setKind] = createSignal<Kind>("all");
  const [folded, setFolded] = createSignal<Record<string, boolean>>({});
  const [renaming, setRenaming] = createSignal<string | null>(null);

  const matches = (i: Item) => (kind() === "all" || i.kind === kind()) && (!query().trim() || i.name.toLowerCase().includes(query().trim().toLowerCase()));
  const byName = (a: Item, b: Item) => a.name.localeCompare(b.name);
  const inGroup = (g: string) =>
    store
      .items()
      .filter((i) => (g === NO_GROUP ? !i.groups.length : i.groups.includes(g)))
      .filter(matches)
      .sort(byName);
  const sections = () => [...store.allGroups(), ...(store.items().some((i) => !i.groups.length) ? [NO_GROUP] : [])];
  const filtering = () => kind() !== "all" || !!query().trim();
  const found = () => store.items().filter(matches).length;

  // ---- drag a look onto a group: it joins that group too
  const [drag, setDrag] = createSignal<{ id: string; x: number; y: number; over: string | null } | null>(null);
  let dragged = false;
  const stop = () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", up);
  };
  let start: { id: string; x: number; y: number } | null = null;
  const groupAt = (x: number, y: number) => {
    const g = document.elementFromPoint(x, y)?.closest("[data-group]")?.getAttribute("data-group");
    return g && g !== NO_GROUP ? g : null;
  };
  const move = (e: PointerEvent) => {
    if (!start) return;
    if (!drag() && Math.hypot(e.clientX - start.x, e.clientY - start.y) < 8) return;
    setDrag({ id: start.id, x: e.clientX, y: e.clientY, over: groupAt(e.clientX, e.clientY) });
  };
  const up = () => {
    const d = drag();
    if (d) {
      // the click that ends a drag must not open the look; a later click (or Enter) must
      dragged = true;
      setTimeout(() => (dragged = false), 0);
      if (d.over) store.addToGroup(d.id, d.over);
    }
    start = null;
    setDrag(null);
    stop();
  };
  const down = (id: string) => (e: PointerEvent) => {
    if (e.button > 0) return;
    dragged = false;
    start = { id, x: e.clientX, y: e.clientY };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  };
  onCleanup(stop);

  return (
    <Region
      id="library"
      title="Library"
      class={`lk-lib ${props.wide ? "is-wide" : ""}`}
      aside={
        <span class="lk-lib-tools">
          <span class="mute">{filtering() ? `${found()} found` : `${store.items().length} looks`}</span>
          <Button class="lk-wide" pressed={props.wide} onClick={() => props.onWide(!props.wide)}>
            {props.wide ? "Fold back" : "Widen"}
          </Button>
        </span>
      }
    >
      <div class="lk-lib-filters">
        <input type="text" placeholder="Search looks" aria-label="Search the library" value={query()} onInput={(e) => setQuery(e.currentTarget.value)} />
        <Segmented
          label="Type"
          value={kind()}
          onChange={setKind}
          options={[
            { value: "all", label: "All" },
            { value: "step", label: "Step" },
            { value: "sequence", label: "Sequence" },
            { value: "pattern", label: "Pattern" },
          ]}
          class="lk-kinds"
        />
        <Button
          class="lk-newgroup"
          onClick={() => {
            const g = store.addGroup();
            setRenaming(g);
          }}
        >
          New group
        </Button>
      </div>
      <NewLook />

      <div class="lk-sections" classList={{ "is-dragging": !!drag() }}>
        <For each={sections()}>
          {(g) => (
            <section class="lk-sec" data-group={g} classList={{ "is-over": drag()?.over === g, "is-folded": !!folded()[g] }}>
              <header class="lk-sec-head">
                <Show
                  when={renaming() === g}
                  fallback={
                    <button type="button" class="lk-sec-fold" aria-expanded={!folded()[g]} onClick={() => setFolded({ ...folded(), [g]: !folded()[g] })}>
                      <svg width="10" height="12" viewBox="0 0 10 12" aria-hidden="true">
                        <path d="M2 1l6 5-6 5" fill="none" stroke="currentColor" stroke-width="2.5" />
                      </svg>
                      <b>{g}</b>
                      <span class="lk-count">{inGroup(g).length}</span>
                    </button>
                  }
                >
                  <form
                    class="lk-sec-rename"
                    onSubmit={(e) => {
                      e.preventDefault();
                      store.renameGroup(g, (e.currentTarget.elements.namedItem("name") as HTMLInputElement).value);
                      setRenaming(null);
                    }}
                  >
                    <input type="text" name="name" aria-label={`New name for ${g}`} value={g} ref={(el) => queueMicrotask(() => el.select())} />
                    <Button type="submit" variant="primary">
                      Save
                    </Button>
                  </form>
                </Show>
                <Show when={g !== NO_GROUP && renaming() !== g}>
                  <button type="button" class="lk-sec-tool" aria-label={`Rename ${g}`} onClick={() => setRenaming(g)}>
                    <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
                      <path d="M2 12l1-4 7-7 3 3-7 7z" fill="none" stroke="currentColor" stroke-width="2" />
                    </svg>
                  </button>
                  <button type="button" class="lk-sec-tool" aria-label={`Delete ${g}. Its looks stay in the library.`} onClick={() => store.deleteGroup(g)}>
                    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
                      <path d="M2 2l8 8M10 2l-8 8" stroke="currentColor" stroke-width="2.5" />
                    </svg>
                  </button>
                </Show>
              </header>
              <Show when={!folded()[g]}>
                <ul class="lk-list" aria-label={g}>
                  <For each={inGroup(g)} fallback={<li class="mute lk-none">{filtering() ? "Nothing here matches." : "Empty. Drag a look here."}</li>}>
                    {(i) => (
                      <li>
                        <button
                          type="button"
                          class="lk-row"
                          aria-current={props.currentId === i.id ? "true" : undefined}
                          onPointerDown={down(i.id)}
                          onClick={() => !dragged && props.onOpen(i.id)}
                        >
                          <KindSymbol kind={i.kind} />
                          <span class="lk-row-text">{i.name}</span>
                          <span class="lk-chips is-small">
                            <For each={store.colorsOf(i.id).slice(0, 4)}>{(c) => <i style={{ background: c }} />}</For>
                          </span>
                          <span class="lk-meas">{store.measureOf(i.id) ? `${store.measureOf(i.id)} b` : ""}</span>
                        </button>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
            </section>
          )}
        </For>
      </div>
      <Show when={drag()}>
        {(d) => (
          <span class="lk-ghost" style={{ left: `${d().x + 12}px`, top: `${d().y + 12}px` }} aria-hidden="true">
            {store.item(d().id)?.name}
            {d().over ? ` into ${d().over}` : ""}
          </span>
        )}
      </Show>
    </Region>
  );
}
