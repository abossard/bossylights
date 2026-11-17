// A tiny hash router: #/live, #/autopilot, ..., #/review.
import { createRoot, createSignal } from "solid-js";

const read = () => location.hash.replace(/^#\/?/, "").split("/")[0] || "live";

export const route = createRoot(() => {
  const [current, setCurrent] = createSignal(read());
  window.addEventListener("hashchange", () => setCurrent(read()));
  return current;
});

export const go = (id: string) => {
  location.hash = `/${id}`;
};
