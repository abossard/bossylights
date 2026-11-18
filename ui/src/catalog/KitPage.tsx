// The page of one primitive: what it is for, every kind, state and colour side by side, the controls
// that are drawn with it, and the owner's rating and comment.
import { For, Show } from "solid-js";
import { kindOf, PRIMITIVE_FOR, PRIMITIVES, primitiveOf, type Control, type Primitive } from "../controls/spec.ts";
import { recipe } from "./compose.ts";
import { reviewOf, type Rating, type Review, type Reviews } from "./review.ts";
import { ReviewBox } from "./ReviewBox.tsx";
import { SHELVES, ShelfView } from "./specimens.tsx";

/** The fixed props of a control as they read on a tag: "mark lamp", "fires down". */
export const propWords = (control: Control): string[] => {
  const r = recipe(control);
  return [...(r.colour === "plain" ? [] : [r.colour]), ...Object.entries(r.props).map(([name, value]) => (value === "yes" ? name : `${name} ${value}`))];
};

export function KitPage(props: { primitive: Primitive; controls: readonly Control[]; reviews: Reviews; review: Review; onRate: (rating: Rating) => void; onComment: (text: string) => void }) {
  const p = props.primitive;
  const users = props.controls.filter((c) => primitiveOf(c.kit) === p);
  const kinds: readonly string[] = PRIMITIVES[p];
  const ofKind = (kind: string) => users.filter((c) => kindOf(c.kit) === kind);

  return (
    <article class="c-page" data-kit-page={p}>
      <header class="c-page-head">
        <p class="c-crumb">Primitive</p>
        <h1>{p}</h1>
        <p class="c-does">{PRIMITIVE_FOR[p]}</p>
        <p class="c-tags">
          <For each={kinds}>{(kind) => <span class="c-tag">{kind}</span>}</For>
          <span class="c-tag">{users.length} controls</span>
        </p>
      </header>

      <For each={SHELVES[p]}>{(shelf) => <ShelfView shelf={shelf} />}</For>

      <h2>Drawn with it</h2>
      <Show when={users.length} fallback={<p class="c-fact">No control is a {p} alone. It stands inside others: a lamp on a toggle, the state of a row, the NOW flag, a reading.</p>}>
        <table class="c-table">
          <thead>
            <tr>
              <th>Kind</th>
              <th>Control</th>
              <th>Where</th>
              <th>Fixed props</th>
              <th>Rated</th>
            </tr>
          </thead>
          <tbody>
            <For each={kinds.length ? kinds : [""]}>
              {(kind) => (
                <For each={ofKind(kind)}>
                  {(c, i) => (
                    <tr>
                      <th>{i() === 0 ? kind || p : ""}</th>
                      <td>
                        <a href={`#/${c.id}`}>{c.name}</a>
                      </td>
                      <td>
                        <span class="c-tag">{c.screen}</span>
                      </td>
                      <td>{propWords(c).join(", ")}</td>
                      <td>{reviewOf(props.reviews, c.id).rating || ""}</td>
                    </tr>
                  )}
                </For>
              )}
            </For>
          </tbody>
        </table>
      </Show>

      <section class="c-review c-review-wide" aria-label="Your review">
        <ReviewBox review={props.review} onRate={props.onRate} onComment={props.onComment} />
      </section>
    </article>
  );
}
