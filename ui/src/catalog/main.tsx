// The storyboard catalog (issue #102). First the ten primitives of the kit, then every control of the
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
import { comment, kitKey, merge, rate, reviewOf, same, type Reviews } from "./review.ts";
import { loadReviews, pullReviews, pushReviews, saveReviews } from "./storage.ts";
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

  const keep = (next: Reviews) => {
    setReviews(next);
    saveReviews(next);
    pushReviews(next);
  };
  // Once, at the start: the file of the repository and this browser meet. The file is written only when this browser adds to it.
  void pullReviews().then((file) => {
    const all = merge(file, reviews());
    if (same(all, file)) {
      setReviews(all);
      saveReviews(all);
    } else keep(all);
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
          {(p) => <KitPage primitive={p} controls={CONTROLS} reviews={reviews()} review={reviewOf(reviews(), kitKey(p))} onRate={(rating) => keep(rate(reviews(), kitKey(p), rating))} onComment={(text) => keep(comment(reviews(), kitKey(p), text))} />}
        </Show>
        <Show when={open()} keyed>
          {(c) => <Page control={c} review={reviewOf(reviews(), c.id)} onRate={(rating) => keep(rate(reviews(), c.id, rating))} onComment={(text) => keep(comment(reviews(), c.id, text))} />}
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
