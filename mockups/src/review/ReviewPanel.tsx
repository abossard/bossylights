import { For, Show } from "solid-js";
import { store } from "../fake/store";
import { Button } from "../kit";
import { ISSUE_URL, screenById } from "../shell/registry";
import { route } from "../shell/router";
import { review, type Verdict } from "./store";

const VERDICTS: { value: Verdict; label: string }[] = [
  { value: "approve", label: "Approve" },
  { value: "change", label: "Change" },
  { value: "rethink", label: "Rethink" },
];

/** The side panel: verdict, rating, comment and region notes for the current screen. */
export function ReviewPanel() {
  const screen = () => screenById(route());
  const r = () => review.get(screen()?.id ?? "");
  const regions = () => review.mounted[screen()?.id ?? ""] ?? [];

  const close = () => {
    review.setPanelOpen(false);
    review.setActiveRegion(null);
  };

  const startCommentMode = () => {
    review.setCommentMode(true);
    review.setActiveRegion(null);
    review.setPanelOpen(false);
  };

  return (
    <Show when={review.panelOpen() && screen()}>
      {(s) => (
        <aside id="review-panel" class="review-panel" aria-label={`Review ${s().title}`}>
          <header class="rp-head">
            <h2 class="display">Review {s().title}</h2>
            <Button variant="quiet" onClick={close}>
              Close
            </Button>
          </header>

          <Show
            when={review.activeRegion()}
            fallback={
              <div class="rp-body">
                <section class="rp-section">
                  <h3 class="rp-h">Stands for</h3>
                  <ul class="rp-issues">
                    <For each={s().issues}>
                      {(i) => (
                        <li>
                          <a href={ISSUE_URL + i.number} target="_blank" rel="noreferrer">
                            #{i.number} {i.title}
                          </a>
                        </li>
                      )}
                    </For>
                  </ul>
                </section>

                <section class="rp-section">
                  <h3 class="rp-h" id="rp-verdict">Verdict</h3>
                  <div class="rp-row" role="group" aria-labelledby="rp-verdict">
                    <For each={VERDICTS}>
                      {(v) => (
                        <Button
                          pressed={r().verdict === v.value}
                          onClick={() => review.setVerdict(s().id, r().verdict === v.value ? null : v.value)}
                        >
                          {v.label}
                        </Button>
                      )}
                    </For>
                  </div>
                </section>

                <section class="rp-section">
                  <h3 class="rp-h" id="rp-rating">Rating</h3>
                  <div class="rp-row" role="group" aria-labelledby="rp-rating">
                    <For each={[1, 2, 3, 4, 5]}>
                      {(n) => (
                        <Button
                          class="rp-star"
                          pressed={r().rating === n}
                          aria-label={`${n} of 5`}
                          onClick={() => review.setRating(s().id, r().rating === n ? null : n)}
                        >
                          {n}
                        </Button>
                      )}
                    </For>
                  </div>
                </section>

                <section class="rp-section">
                  <label class="rp-h" for="rp-comment">
                    Comment
                  </label>
                  <textarea
                    id="rp-comment"
                    rows={5}
                    placeholder="What works, what does not, what is missing"
                    value={r().comment}
                    onInput={(e) => review.setComment(s().id, e.currentTarget.value)}
                  />
                  <Show when={r().previous}>
                    <p class="rp-previous">
                      <b>Earlier you said</b> {r().previous}
                    </p>
                    <p class="mute">This screen was rebuilt after that, so it is open again.</p>
                  </Show>
                </section>

                <section class="rp-section">
                  <h3 class="rp-h">States to look at</h3>
                  <div class="rp-states">
                    <label>
                      <input
                        type="checkbox"
                        checked={!store.state.lightsConnected}
                        onChange={(e) => store.setLightsConnected(!e.currentTarget.checked)}
                      />
                      No lights connected
                    </label>
                    <label>
                      <input
                        type="checkbox"
                        checked={!store.state.cuedDeckLoaded}
                        onChange={(e) => store.setCuedDeckLoaded(!e.currentTarget.checked)}
                      />
                      Nothing loaded on the second deck
                    </label>
                  </div>
                  <p class="mute">LIVE on and off: use the switch in the top bar.</p>
                </section>

                <section class="rp-section">
                  <div class="rp-h-row">
                    <h3 class="rp-h">Region notes</h3>
                    <Button variant="primary" onClick={startCommentMode}>
                      Comment mode
                    </Button>
                  </div>
                  <p class="mute">Comment mode outlines every region. Pick one to leave a note on it.</p>
                  <ul class="rp-regions">
                    <For each={regions()}>
                      {(reg) => {
                        const note = () => r().regions[reg.id]?.note.trim() ?? "";
                        return (
                          <li>
                            <button type="button" class="rp-region" onClick={() => review.openRegion(reg.id, reg.title)}>
                              <b>{reg.title}</b>
                              <span class="mute">{note() || "No note yet"}</span>
                            </button>
                          </li>
                        );
                      }}
                    </For>
                  </ul>
                </section>
              </div>
            }
          >
            {(reg) => (
              <div class="rp-body">
                <section class="rp-section">
                  <label class="rp-h" for="rp-region-note">
                    Note on {reg().title}
                  </label>
                  <textarea
                    id="rp-region-note"
                    rows={8}
                    ref={(el) => queueMicrotask(() => el.focus())}
                    placeholder="What should change in this region"
                    value={r().regions[reg().id]?.note ?? ""}
                    onInput={(e) => review.setRegionNote(s().id, reg().id, reg().title, e.currentTarget.value)}
                  />
                  <p class="mute">Saved as you type.</p>
                </section>
                <div class="rp-row">
                  <Button
                    variant="primary"
                    onClick={() => {
                      review.setActiveRegion(null);
                      if (review.commentMode()) review.setPanelOpen(false);
                    }}
                  >
                    {review.commentMode() ? "Back to regions" : "Done"}
                  </Button>
                </div>
              </div>
            )}
          </Show>
        </aside>
      )}
    </Show>
  );
}
