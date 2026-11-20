// The one place the catalog keeps reviews outside itself: the browser's store, and the file of the
// repository that the dev server writes (`reviews-file.mjs`). Actions, kept thin.
// One review at a time: a second tab never writes over what the first one rated.
import { parse, put, reviewOf, type Review, type Reviews } from "./review.ts";

const KEY = "bossylights.catalog.reviews.v1";
const FILE = "/reviews.json";

export function loadReviews(): Reviews {
  try {
    return parse(localStorage.getItem(KEY));
  } catch {
    // a browser with its store switched off: the reviews live until the page closes
    return {};
  }
}

/** Puts one review into the store as it is now, so the reviews of another tab stay. False: the store took nothing. */
export function storeReview(id: string, review: Review | undefined): boolean {
  try {
    const now = parse(localStorage.getItem(KEY));
    localStorage.setItem(KEY, JSON.stringify(put(now, id, review ?? reviewOf({}, id))));
    return true;
  } catch {
    return false;
  }
}

/** What another tab's store event changed: the reviews before and after. Undefined: not the reviews. */
export function storeChange(event: StorageEvent): readonly [Reviews, Reviews] | undefined {
  return event.key === KEY ? [parse(event.oldValue), parse(event.newValue)] : undefined;
}

/** The reviews in the file of the repository. Undefined where no server keeps one: a built catalog. */
export async function pullReviews(): Promise<Reviews | undefined> {
  try {
    const response = await fetch(FILE);
    return response.ok ? parse(await response.text()) : undefined;
  } catch {
    return undefined;
  }
}

/** Writes one review into the file. A review of none takes it out. False: the server did not take it. */
export async function sendReview(id: string, review: Review | undefined): Promise<boolean> {
  try {
    return (await fetch(FILE, { method: "PUT", body: JSON.stringify({ id, review: review ?? null }) })).ok;
  } catch {
    return false;
  }
}
