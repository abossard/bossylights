// The one place the catalog touches the browser's store. An action, kept thin.
import { parse, type Reviews } from "./review.ts";

const KEY = "bossylights.catalog.reviews.v1";

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
