// The owner's rating and comment on one thing of the catalog: a control or a primitive.
import { For } from "solid-js";
import type { Rating, Review } from "./review.ts";

const RATINGS: readonly Rating[] = [1, 2, 3, 4, 5];

export function ReviewBox(props: { review: Review; onRate: (rating: Rating) => void; onComment: (text: string) => void }) {
  return (
    <>
      <h2>Your review</h2>
      <div class="c-rating" role="group" aria-label="Rating, 1 to 5">
        <For each={RATINGS}>
          {(rating) => (
            <button type="button" class="c-rate" aria-pressed={props.review.rating === rating} aria-label={`${rating} of 5`} onClick={() => props.onRate(rating)}>
              {rating}
            </button>
          )}
        </For>
        <span class="c-rating-says">{props.review.rating === 0 ? "Not rated" : `${props.review.rating} of 5`}</span>
      </div>
      <label class="c-comment">
        <span>What works, what does not?</span>
        <textarea rows={3} value={props.review.comment} onInput={(e) => props.onComment(e.currentTarget.value)} />
      </label>
    </>
  );
}
