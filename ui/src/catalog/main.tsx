// The storyboard catalog: every control of the behaviour table on its own page (issue #102).
// Its own app: `npm run catalog`. State flows one way: the address and the reviews in, pages out.
import { createEffect, createMemo, createSignal, For, on, onCleanup, Show } from "solid-js";
import { render } from "solid-js/web";
import { CONTROLS } from "../controls/table.ts";
import { TextField } from "../kit/index.ts";
import { byScreen, search } from "./frames.ts";
import { Home } from "./Home.tsx";
import { Page } from "./Page.tsx";
import { comment, rate, reviewOf, type Reviews } from "./review.ts";
import { loadReviews, saveReviews } from "./storage.ts";
import "./catalog.css";

/** The control an address names: "#/top.blackout". */
const named = (hash: string) => CONTROLS.find((c) => c.id === decodeURIComponent(hash.replace(/^#\/?/, "")));

function Catalog() {
  const [hash, setHash] = createSignal(location.hash);
  const [query, setQuery] = createSignal("");
  const [reviews, setReviews] = createSignal<Reviews>(loadReviews());

  const follow = () => setHash(location.hash);
  window.addEventListener("hashchange", follow);
  onCleanup(() => window.removeEventListener("hashchange", follow));

  const keep = (next: Reviews) => {
    setReviews(next);
    saveReviews(next);
  };
  const found = createMemo(() => search(CONTROLS, query()));
  const open = createMemo(() => named(hash()));

  // A new page starts at its top.
  let main: HTMLElement | undefined;
  createEffect(on(open, () => main?.scrollTo(0, 0), { defer: true }));

  return (
    <div class="c-app">
      <nav class="c-index" aria-label="Every control">
        <a class="c-home" href="#/">
          Storyboard catalog
        </a>
        <TextField label="Search" value={query()} placeholder="name, screen, command, kind" live onText={setQuery} />
        <p class="c-count" data-count>
          {found().length} of {CONTROLS.length} controls
        </p>
        <div class="c-list">
          <For each={byScreen(found())}>
            {([screen, controls]) => (
              <section>
                <h2>{screen}</h2>
                <For each={controls}>
                  {(c) => (
                    <a href={`#/${c.id}`} aria-current={open()?.id === c.id ? "page" : undefined} data-link={c.id}>
                      <span class="c-link-name">{c.name}</span>
                      <Show when={c.first.verdict !== "keep"}>
                        <span class="c-tag" data-verdict={c.first.verdict}>
                          {c.first.verdict}
                        </span>
                      </Show>
                      <Show when={reviewOf(reviews(), c.id).rating}>
                        <span class="c-rated" aria-label={`rated ${reviewOf(reviews(), c.id).rating} of 5`}>
                          {reviewOf(reviews(), c.id).rating}
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
        <Show when={open()} keyed fallback={<Home controls={CONTROLS} reviews={reviews()} />}>
          {(c) => <Page control={c} review={reviewOf(reviews(), c.id)} onRate={(rating) => keep(rate(reviews(), c.id, rating))} onComment={(text) => keep(comment(reviews(), c.id, text))} />}
        </Show>
      </main>
    </div>
  );
}

const root = document.getElementById("root");
if (root) render(() => <Catalog />, root);
