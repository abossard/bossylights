// The one place the catalog keeps reviews outside itself: the browser's store, and the file of the
// repository that the dev server writes (`reviews-file.mjs`). Actions, kept thin.
import { parse, type Reviews } from "./review.ts";

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

export function saveReviews(reviews: Reviews): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(reviews));
  } catch {
    // see loadReviews
  }
}

/** The reviews in the file of the repository. None where no server keeps one: the headless run, a built catalog. */
export async function pullReviews(): Promise<Reviews> {
  try {
    return parse(await (await fetch(FILE)).text());
  } catch {
    return {};
  }
}

export function pushReviews(reviews: Reviews): void {
  fetch(FILE, { method: "PUT", body: JSON.stringify(reviews) }).catch(() => undefined);
}
