// What a text field says under itself, as one state machine (docs/design/ui-states.md). A calculation:
// what must happen to the text and the show is returned as an effect, done by Field.tsx.

/** Clean: nothing to say. Refused: the last text did not fit, and why. Unsaved: a text that fits was left without Enter. */
export type FieldState = { is: "clean" } | { is: "refused"; reason: string } | { is: "unsaved" };

/**
 * typed: a letter changed. enter, leave: the text there then, judged by `judge`. escape: put the old value back.
 * `saves`: whether leaving saves, as Enter does, or only marks the text unsaved.
 */
export type FieldEvent =
  | { is: "typed" }
  | { is: "enter"; text: Judged }
  | { is: "leave"; text: Judged; saves: boolean }
  | { is: "escape" };

/** A text as the field sees it: the old value again (or empty), refused with a reason, or one that fits. */
export type Judged = { is: "same" } | { is: "refused"; reason: string } | { is: "fits"; text: string };

/** send: the text goes out. putBack: the input shows the old value again. */
export type FieldEffect = { is: "none" } | { is: "send"; text: string } | { is: "putBack" };

export const CLEAN: FieldState = { is: "clean" };

/** Judges a text against the value it replaces. `refuse` says why a text does not fit, or nothing. */
export function judge(input: string, value: string, refuse?: (text: string) => string | undefined): Judged {
  const text = input.trim();
  if (text === "" || text === value) return { is: "same" };
  const reason = refuse?.(text);
  return reason === undefined ? { is: "fits", text } : { is: "refused", reason };
}

/** Every transition. The state before never changes the outcome: it only says what is shown until the next event. */
export function fieldStep(_state: FieldState, event: FieldEvent): [FieldState, FieldEffect] {
  switch (event.is) {
    case "typed":
      return [CLEAN, { is: "none" }];
    case "escape":
      return [CLEAN, { is: "putBack" }];
    case "enter":
      return settle(event.text, false);
    case "leave":
      if (event.saves || event.text.is !== "fits") return settle(event.text, true);
      return [{ is: "unsaved" }, { is: "none" }];
  }
}

/** Enter, or leaving. A refused text stays in the input after Enter, to be mended; leaving puts the old value back. */
function settle(text: Judged, leaving: boolean): [FieldState, FieldEffect] {
  switch (text.is) {
    case "same":
      return [CLEAN, { is: "putBack" }];
    case "refused":
      return [{ is: "refused", reason: text.reason }, leaving ? { is: "putBack" } : { is: "none" }];
    case "fits":
      return [CLEAN, { is: "send", text: text.text }];
  }
}
