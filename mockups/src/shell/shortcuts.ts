// 1 to 6 switch workspace, Space holds blackout, Cmd/Ctrl+Z undoes.
import { store } from "../fake/store";
import { screens } from "./registry";
import { go } from "./router";

const typing = (t: EventTarget | null) =>
  t instanceof HTMLElement && (t.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName) && !(t instanceof HTMLInputElement && t.type === "checkbox"));

export function installShortcuts() {
  window.addEventListener("keydown", (e) => {
    if (typing(e.target)) return;
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "z" && !e.shiftKey) {
      e.preventDefault();
      store.undo();
      return;
    }
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.code === "Space") {
      e.preventDefault();
      if (!e.repeat) store.setBlackoutHold(true);
      return;
    }
    const n = Number(e.key);
    if (Number.isInteger(n) && n >= 1 && n <= 6) {
      const s = screens.find((x) => x.order === n);
      if (s) go(s.id);
    }
  });
  window.addEventListener("keyup", (e) => {
    if (e.code === "Space" && !typing(e.target)) {
      e.preventDefault();
      store.setBlackoutHold(false);
    }
  });
  window.addEventListener("blur", () => store.setBlackoutHold(false));
}
