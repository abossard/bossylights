// The owner's rating and comment per control: data, and the calculations over it.
// `storage.ts` keeps them in the browser. The export is what goes back into the issue.
import type { Control } from "../controls/spec.ts";

export type Rating = 0 | 1 | 2 | 3 | 4 | 5;

/** `rating` 0: not rated yet. */
export interface Review {
  rating: Rating;
  comment: string;
}

/** By the stable name of the control. A control with no rating and no comment is not in it. */
export type Reviews = Readonly<Record<string, Review>>;

const NONE: Review = { rating: 0, comment: "" };

export const reviewOf = (reviews: Reviews, id: string): Review => reviews[id] ?? NONE;

function put(reviews: Reviews, id: string, review: Review): Reviews {
  const { [id]: _old, ...rest } = reviews;
  return review.rating === 0 && review.comment.trim() === "" ? rest : { ...rest, [id]: review };
}

/** Rates a control. The same rating again takes it back. */
export function rate(reviews: Reviews, id: string, rating: Rating): Reviews {
  const old = reviewOf(reviews, id);
  return put(reviews, id, { ...old, rating: old.rating === rating ? 0 : rating });
}

export function comment(reviews: Reviews, id: string, text: string): Reviews {
  return put(reviews, id, { ...reviewOf(reviews, id), comment: text });
}

const isRating = (value: unknown): value is Rating => typeof value === "number" && Number.isInteger(value) && value >= 0 && value <= 5;

/** Reviews from stored text. Anything that is not a review is left out: a broken store is an empty one. */
export function parse(text: string | null): Reviews {
  let raw: unknown;
  try {
    raw = JSON.parse(text ?? "{}");
  } catch {
    return {};
  }
  if (typeof raw !== "object" || raw === null) return {};
  const reviews: Record<string, Review> = {};
  for (const [id, value] of Object.entries(raw)) {
    if (typeof value !== "object" || value === null) continue;
    const { rating, comment: text } = value as { rating?: unknown; comment?: unknown };
    if (isRating(rating) && typeof text === "string") reviews[id] = { rating, comment: text };
  }
  return reviews;
}

const cell = (text: string) => text.replace(/\|/g, "\\|").replace(/\s+/g, " ").trim();

/** The reviews as a Markdown table, in the order of the behaviour table, for the issue. */
export function asMarkdown(reviews: Reviews, controls: readonly Control[]): string {
  const rows = controls.filter((c) => c.id in reviews).map((c) => {
    const r = reviewOf(reviews, c.id);
    return `| ${cell(c.name)} | \`${c.id}\` | ${c.screen} | ${r.rating === 0 ? "" : `${r.rating} of 5`} | ${cell(r.comment)} |`;
  });
  return [`Catalog review: ${rows.length} of ${controls.length} controls`, "", "| Control | Name | Screen | Rating | Comment |", "|---|---|---|---|---|", ...rows].join("\n");
}
