import { ISSUE_URL, screens } from "../shell/registry";
import { QUESTIONS } from "./questions";
import { review, type Verdict } from "./store";

const VERDICT: Record<Verdict, string> = { approve: "Approve", change: "Change", rethink: "Rethink" };
export const verdictLabel = (v: Verdict | null) => (v ? VERDICT[v] : "Not reviewed");

export function exportData() {
  return {
    exportedAt: new Date().toISOString(),
    screens: screens.map((s) => {
      const r = review.get(s.id);
      return {
        id: s.id,
        title: s.title,
        issues: s.issues.map((i) => ({ ...i, url: ISSUE_URL + i.number })),
        verdict: r.verdict,
        rating: r.rating,
        comment: r.comment,
        regionNotes: Object.entries(r.regions)
          .filter(([, n]) => n.note.trim())
          .map(([region, n]) => ({ region, title: n.title, note: n.note })),
      };
    }),
    questions: QUESTIONS.map((q) => {
      const a = review.answerOf(q.id);
      return {
        id: q.id,
        source: q.source,
        number: q.number,
        question: q.question,
        optionA: q.a || null,
        optionB: q.b || null,
        choice: a.choice,
        answer: a.choice === "A" ? q.a : a.choice === "B" ? q.b : null,
        text: a.text,
      };
    }),
  };
}

export function downloadJson() {
  const blob = new Blob([JSON.stringify(exportData(), null, 2)], { type: "application/json" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = `bossylights-review-${new Date().toISOString().slice(0, 10)}.json`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}

export function toMarkdown(): string {
  const data = exportData();
  const lines = ["# Bossylights mockup review", "", `Exported ${data.exportedAt.slice(0, 16).replace("T", " ")}`, ""];
  for (const s of data.screens) {
    lines.push(`## ${s.title}`, "");
    lines.push(`- Issues: ${s.issues.map((i) => `#${i.number} ${i.title}`).join(", ")}`);
    lines.push(`- Verdict: ${verdictLabel(s.verdict)}`);
    lines.push(`- Rating: ${s.rating ? `${s.rating}/5` : "none"}`);
    if (s.comment.trim()) lines.push(`- Comment: ${s.comment.trim().replace(/\n+/g, " ")}`);
    if (s.regionNotes.length) {
      lines.push("- Region notes:");
      for (const n of s.regionNotes) lines.push(`  - ${n.title}: ${n.note.trim().replace(/\n+/g, " ")}`);
    }
    lines.push("");
  }
  lines.push("## Questions", "");
  for (const q of data.questions) {
    const said = [q.choice ? `${q.choice}: ${q.answer}` : "", q.text.trim().replace(/\n+/g, " ")].filter(Boolean).join(". ");
    lines.push(`- ${q.source} ${q.number}, ${q.question} ${said || "not answered"}`);
  }
  lines.push("");
  return lines.join("\n");
}

/** Copies, with a fallback for browsers that block the clipboard API. */
export async function copyMarkdown(): Promise<boolean> {
  const text = toMarkdown();
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    document.body.append(ta);
    ta.select();
    const ok = document.execCommand("copy");
    ta.remove();
    return ok;
  }
}
