// The front page of the catalog: how to read a page, how far the review is, and the reviews to take away.
import { For } from "solid-js";
import { SITUATIONS } from "../controls/situations.ts";
import type { Control } from "../controls/spec.ts";
import { byScreen } from "./frames.ts";
import { asMarkdown, reviewOf, type Reviews } from "./review.ts";

const VERDICTS = ["keep", "change", "remove"] as const;

export function Home(props: { controls: readonly Control[]; reviews: Reviews }) {
  const rated = () => props.controls.filter((c) => reviewOf(props.reviews, c.id).rating > 0).length;
  const copy = (e: MouseEvent & { currentTarget: HTMLButtonElement }) => {
    const text = e.currentTarget.parentElement?.querySelector("textarea");
    text?.select();
    void navigator.clipboard?.writeText(text?.value ?? "");
  };
  return (
    <article class="c-page c-homepage">
      <header class="c-page-head">
        <h1>Storyboard catalog</h1>
        <p class="c-does">Every control, alone, in every situation. Play a frame: the command shows as it fires. Rate each one.</p>
      </header>

      <div class="c-numbers">
        <p>
          <b>{props.controls.length}</b> controls
        </p>
        <p>
          <b>{SITUATIONS.length}</b> situations each
        </p>
        <For each={VERDICTS}>
          {(verdict) => (
            <p>
              <b data-verdict={verdict}>{props.controls.filter((c) => c.first.verdict === verdict).length}</b> {verdict}
            </p>
          )}
        </For>
        <p data-rated>
          <b>{rated()}</b> rated
        </p>
      </div>

      <h2>A page, frame by frame</h2>
      <table class="c-table">
        <tbody>
          <tr>
            <th>Alone</th>
            <td>The control at real size, in the usual state</td>
          </tr>
          <tr>
            <th>In each situation</th>
            <td>One frame per column of the behaviour table. It works, it is off and says why, or it is not there</td>
          </tr>
          <tr>
            <th>Its states</th>
            <td>One frame per state it can show</td>
          </tr>
          <tr>
            <th>Under a frame</th>
            <td>The command, as it fires. Hover it for its fields</td>
          </tr>
          <tr>
            <th>Your review</th>
            <td>1 to 5 and a comment per control. Kept in this browser, taken away below</td>
          </tr>
        </tbody>
      </table>

      <h2>By screen</h2>
      <table class="c-table">
        <thead>
          <tr>
            <th>Screen</th>
            <th>Controls</th>
            <th>Change</th>
            <th>Remove</th>
            <th>Rated</th>
          </tr>
        </thead>
        <tbody>
          <For each={byScreen(props.controls)}>
            {([screen, controls]) => (
              <tr>
                <th>
                  <a href={`#/${controls[0]?.id ?? ""}`}>{screen}</a>
                </th>
                <td>{controls.length}</td>
                <td>{controls.filter((c) => c.first.verdict === "change").length}</td>
                <td>{controls.filter((c) => c.first.verdict === "remove").length}</td>
                <td>{controls.filter((c) => reviewOf(props.reviews, c.id).rating > 0).length}</td>
              </tr>
            )}
          </For>
        </tbody>
      </table>

      <h2>Take the reviews away</h2>
      <div class="c-export">
        <textarea readonly rows={8} data-export value={asMarkdown(props.reviews, props.controls)} />
        <button type="button" class="c-rate c-copy" onClick={copy}>
          Copy as Markdown
        </button>
      </div>
    </article>
  );
}
