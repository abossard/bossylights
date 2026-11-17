import { createSignal } from "solid-js";
import { Button, Table } from "../kit";
import { screens, type ScreenModule } from "../shell/registry";
import { go } from "../shell/router";
import { copyMarkdown, downloadJson, verdictLabel } from "./export";
import { review } from "./store";

/** The ninth tab: every screen with its verdict, rating and notes, plus export. */
export function ReviewScreen() {
  const [message, setMessage] = createSignal("");

  const copy = async () => setMessage((await copyMarkdown()) ? "Copied as Markdown." : "Copy failed: use Export instead.");
  const exportFile = () => {
    downloadJson();
    setMessage("Exported as a JSON file.");
  };

  const open = (s: ScreenModule) => {
    go(s.id);
    review.setPanelOpen(true);
  };

  return (
    <div class="review-screen">
      <header class="review-screen-head">
        <h1 class="display">Review</h1>
        <p class="mute">Your verdicts stay in this browser until you export them.</p>
        <div class="review-actions">
          <span class="mute" role="status">{message()}</span>
          <Button onClick={copy}>Copy as Markdown</Button>
          <Button variant="primary" onClick={exportFile}>
            Export
          </Button>
        </div>
      </header>
      <Table
        caption="Review of every screen"
        rows={screens}
        rowKey={(s) => s.id}
        onRowClick={open}
        columns={[
          { key: "title", title: "Screen", width: "160px", render: (s) => <span class="display rs-title">{s.order}. {s.title}</span> },
          { key: "issues", title: "Stands for", render: (s) => <span class="rs-issues">{s.issues.map((i) => `#${i.number} ${i.title}`).join(", ")}</span> },
          {
            key: "verdict",
            title: "Verdict",
            width: "130px",
            render: (s) => <span class={`rs-verdict rs-${review.get(s.id).verdict ?? "none"}`}>{verdictLabel(review.get(s.id).verdict)}</span>,
          },
          { key: "rating", title: "Rating", width: "80px", align: "right", render: (s) => (review.get(s.id).rating ? `${review.get(s.id).rating}/5` : "none") },
          { key: "notes", title: "Notes", width: "80px", align: "right", render: (s) => String(review.noteCount(s.id)) },
        ]}
      />
    </div>
  );
}
