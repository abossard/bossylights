// The owner's rating and comment per control and per primitive: data, and the calculations over it.
// `storage.ts` keeps them in the browser. The export is what goes back into the issue.
import type { Control } from "../controls/spec.ts";

export type Rating = 0 | 1 | 2 | 3 | 4 | 5;

/** `rating` 0: not rated yet. */
export interface Review {
  rating: Rating;
  comment: string;
}

/** By the stable name of the control, or `kit:` and the name of a primitive. What has no rating and no comment is not in it. */
export type Reviews = Readonly<Record<string, Review>>;

const NONE: Review = { rating: 0, comment: "" };

export const reviewOf = (reviews: Reviews, id: string): Review => reviews[id] ?? NONE;

/** The reviews with one put in. A review with no rating and no comment takes it out. */
export function put(reviews: Reviews, id: string, review: Review): Reviews {
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

/** The fields of a review a tab changed: what it writes to the file. */
export type ReviewField = keyof Review;

/** The part of one review that names only these fields. */
export const partOf = (reviews: Reviews, id: string, fields: readonly ReviewField[]): Partial<Review> => Object.fromEntries(fields.map((f) => [f, reviewOf(reviews, id)[f]]));

/**
 * The file answered a write of these fields of one review: the file's copy comes in, the fields this tab wrote stay
 * as this tab has them now. A rating another tab took back stays taken back; what was typed since stays typed.
 */
export const written = (now: Reviews, file: Reviews, id: string, fields: readonly ReviewField[]): Reviews => put(now, id, { ...reviewOf(file, id), ...partOf(now, id, fields) });

/** The file of the repository and this browser meet: a control reviewed in both keeps the browser's. */
export const merge = (file: Reviews, browser: Reviews): Reviews => ({ ...file, ...browser });

/** The reviews that differ between two sets of reviews. */
export const changed = (before: Reviews, after: Reviews): string[] =>
  [...new Set([...Object.keys(before), ...Object.keys(after)])].filter((id) => !same({ [id]: reviewOf(before, id) }, { [id]: reviewOf(after, id) }));

/** Another tab changed the store from `before` to `after`: only the reviews it changed come in, the rest of this tab stays. */
export function arrived(now: Reviews, before: Reviews, after: Reviews): Reviews {
  return changed(before, after).reduce((next, id) => put(next, id, reviewOf(after, id)), now);
}

export function same(a: Reviews, b: Reviews): boolean {
  const ids = Object.keys(a);
  return ids.length === Object.keys(b).length && ids.every((id) => id in b && a[id].rating === b[id].rating && a[id].comment === b[id].comment);
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

/** The name a primitive is reviewed under. Control names never start like this. */
export const kitKey = (primitive: string) => `kit:${primitive}`;

/** The reviews as a Markdown table, primitives first, then controls in the order of the behaviour table, for the issue. */
export function asMarkdown(reviews: Reviews, controls: readonly Control[], primitives: readonly string[]): string {
  const line = (name: string, id: string, where: string) => {
    const r = reviewOf(reviews, id);
    return `| ${cell(name)} | \`${id}\` | ${where} | ${r.rating === 0 ? "" : `${r.rating} of 5`} | ${cell(r.comment)} |`;
  };
  const kit = primitives.filter((p) => kitKey(p) in reviews).map((p) => line(p, kitKey(p), "Kit"));
  const rows = controls.filter((c) => c.id in reviews).map((c) => line(c.name, c.id, c.screen));
  const head = `Catalog review: ${rows.length} of ${controls.length} controls, ${kit.length} of ${primitives.length} primitives`;
  return [head, "", "| Reviewed | Name | Where | Rating | Comment |", "|---|---|---|---|---|", ...kit, ...rows].join("\n");
}
