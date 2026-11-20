// The storyboard catalog (issue #102). First the primitives of the kit, then every control of the
// behaviour table under the primitive it is drawn with. Where a control lives on screen is a tag.
// Its own app: `npm run catalog`. State flows one way: the address and the reviews in, pages out.
import { createEffect, createMemo, createSignal, For, on, onCleanup, Show } from "solid-js";
import { render } from "solid-js/web";
import { PRIMITIVES, primitiveOf, type Primitive } from "../controls/spec.ts";
import { CONTROLS } from "../controls/table.ts";
import { Field, named, Picker } from "../kit/index.ts";
import { byFunction } from "./compose.ts";
import { screensOf, search } from "./frames.ts";
import { Home } from "./Home.tsx";
import { KitPage } from "./KitPage.tsx";
import { Page } from "./Page.tsx";
import { arrived, changed, comment, kitKey, merge, partOf, rate, reviewOf, written, type ReviewField, type Reviews } from "./review.ts";
import { loadReviews, pullReviews, sendReview, storeChange, storeReview } from "./storage.ts";
import "./catalog.css";

const KIT = Object.keys(PRIMITIVES) as Primitive[];
const EVERY_SCREEN = "Every screen";

const path = (hash: string) => decodeURIComponent(hash.replace(/^#\/?/, ""));
/** The control an address names: "#/top.blackout". */
const control = (hash: string) => CONTROLS.find((c) => c.id === path(hash));
/** The primitive an address names: "#/kit/Button". */
const primitive = (hash: string) => KIT.find((p) => `kit/${p}` === path(hash));

function Catalog() {
  const [hash, setHash] = createSignal(location.hash);
  const [query, setQuery] = createSignal("");
  const [screen, setScreen] = createSignal(EVERY_SCREEN);
  const [reviews, setReviews] = createSignal<Reviews>(loadReviews());

  const follow = () => setHash(location.hash);
  window.addEventListener("hashchange", follow);
  onCleanup(() => window.removeEventListener("hashchange", follow));

  // Why the last review was not kept. Empty: it was.
  const [unsaved, setUnsaved] = createSignal("");
  // How often another tab changed each review: the news a write's answer must not be older than.
  const heard = new Map<string, number>();
  // Whether a server keeps the file: a built catalog has none, and nothing is written there.
  let hasFile = false;
  /**
   * One review is kept: in this browser's store as it is now, and in the file only the fields this tab changed.
   * The file answers with what it holds: another tab may have changed the review there, and that comes in here too.
   */
  const send = (id: string, fields: readonly ReviewField[]) => {
    const stored = storeReview(id, reviews()[id]);
    setUnsaved(stored ? "" : "Not kept in this browser: its store is off");
    if (!hasFile) return;
    const news = heard.get(id) ?? 0;
    void sendReview(id, partOf(reviews(), id, fields)).then((file) => {
      if (file === undefined) return setUnsaved("Not written to the file: the catalog's server did not take it");
      // Another tab's news of this review came after the write left: the answer may be older than it.
      if (file === null || (heard.get(id) ?? 0) !== news) return;
      setReviews(written(reviews(), file, id, fields));
      storeReview(id, reviews()[id]);
    });
  };
  const keep = (id: string, next: Reviews, field: ReviewField) => {
    setReviews(next);
    send(id, [field]);
  };
  // Another tab kept a review: this one shows it too, and keeps what the other tab did not touch (a review only in the file).
  const fromAnotherTab = (event: StorageEvent) => {
    const change = storeChange(event);
    if (!change) return;
    for (const id of changed(...change)) heard.set(id, (heard.get(id) ?? 0) + 1);
    setReviews(arrived(reviews(), ...change));
  };
  window.addEventListener("storage", fromAnotherTab);
  onCleanup(() => window.removeEventListener("storage", fromAnotherTab));
  // Once, at the start: the file of the repository and this browser meet. What only this browser has is written to the file.
  void pullReviews().then((file) => {
    if (file === undefined) return;
    hasFile = true;
    const browser = reviews();
    setReviews(merge(file, browser));
    for (const id of Object.keys(browser)) if (JSON.stringify(browser[id]) !== JSON.stringify(file[id])) send(id, ["rating", "comment"]);
  });

  const found = createMemo(() => search(CONTROLS, query()).filter((c) => screen() === EVERY_SCREEN || c.screen === screen()));
  const open = createMemo(() => control(hash()));
  const kit = createMemo(() => primitive(hash()));
  const rated = (id: string) => reviewOf(reviews(), id).rating;

  // A new page starts at its top.
  let main: HTMLElement | undefined;
  createEffect(on([open, kit], () => main?.scrollTo(0, 0), { defer: true }));

  return (
    <div class="c-app">
      <nav class="c-index" aria-label="The kit and every control">
        <a class="c-home" href="#/">
          Storyboard catalog
        </a>
        <Field kind="search" label="Search" value={query()} placeholder="name, primitive, screen, command" onText={setQuery} />
        <Picker kind="list" label="Where it lives" options={named([EVERY_SCREEN, ...screensOf(CONTROLS)])} picked={screen()} onPick={setScreen} />
        <p class="c-count" data-count>
          {found().length} of {CONTROLS.length} controls
        </p>
        <div class="c-list">
          <section>
            <h2>Primitives</h2>
            <For each={KIT}>
              {(p) => (
                <a href={`#/kit/${p}`} aria-current={kit() === p ? "page" : undefined} data-kit-link={p}>
                  <span class="c-link-name">{p}</span>
                  <span class="c-link-tag">{CONTROLS.filter((c) => primitiveOf(c.kit) === p).length || ""}</span>
                  <Show when={rated(kitKey(p))}>
                    <span class="c-rated" aria-label={`rated ${rated(kitKey(p))} of 5`}>
                      {rated(kitKey(p))}
                    </span>
                  </Show>
                </a>
              )}
            </For>
          </section>
          <For each={byFunction(found(), KIT)}>
            {(group) => (
              <section>
                <h2>
                  {group.primitive}
                  <Show when={group.kind}>
                    <span class="c-kind"> {group.kind}</span>
                  </Show>
                </h2>
                <For each={group.controls}>
                  {(c) => (
                    <a href={`#/${c.id}`} aria-current={open()?.id === c.id ? "page" : undefined} data-link={c.id}>
                      <span class="c-link-name">{c.name}</span>
                      <Show when={c.first.verdict !== "keep"}>
                        <span class="c-tag" data-verdict={c.first.verdict}>
                          {c.first.verdict}
                        </span>
                      </Show>
                      <span class="c-link-tag">{c.screen}</span>
                      <Show when={rated(c.id)}>
                        <span class="c-rated" aria-label={`rated ${rated(c.id)} of 5`}>
                          {rated(c.id)}
                        </span>
                      </Show>
                    </a>
                  )}
                </For>
              </section>
            )}
          </For>
        </div>
      </nav>
      <main class="c-main" ref={(el) => (main = el)}>
        <Show when={kit()} keyed>
          {(p) => <KitPage primitive={p} controls={CONTROLS} reviews={reviews()} review={reviewOf(reviews(), kitKey(p))} unsaved={unsaved()} onRate={(rating) => keep(kitKey(p), rate(reviews(), kitKey(p), rating), "rating")} onComment={(text) => keep(kitKey(p), comment(reviews(), kitKey(p), text), "comment")} />}
        </Show>
        <Show when={open()} keyed>
          {(c) => <Page control={c} review={reviewOf(reviews(), c.id)} unsaved={unsaved()} onRate={(rating) => keep(c.id, rate(reviews(), c.id, rating), "rating")} onComment={(text) => keep(c.id, comment(reviews(), c.id, text), "comment")} />}
        </Show>
        <Show when={!kit() && !open()}>
          <Home controls={CONTROLS} primitives={KIT} reviews={reviews()} />
        </Show>
      </main>
    </div>
  );
}

const root = document.getElementById("root");
if (root) render(() => <Catalog />, root);
