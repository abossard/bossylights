// Review state: verdict, rating, comment and region notes per screen.
// Kept in localStorage until exported.
import { createContext, createEffect, createRoot, createSignal, useContext } from "solid-js";
import { createStore } from "solid-js/store";

export type Verdict = "approve" | "change" | "rethink";

export interface RegionNote {
  title: string;
  note: string;
}

export interface ScreenReview {
  verdict: Verdict | null;
  rating: number | null;
  comment: string;
  regions: Record<string, RegionNote>;
  /** the comment from the review before the screen was rebuilt, kept to read */
  previous?: string;
}

const KEY = "bossylights.review.v1";

/** An answer to one review question: A, B or none, plus free words. */
export interface Answer {
  choice: "A" | "B" | null;
  text: string;
}

const empty = (): ScreenReview => ({ verdict: null, rating: null, comment: "", regions: {} });

/** Screens rebuilt after review 2: they show as not reviewed again. What review 2 said stays readable. */
const REBUILT = ["console", "songs", "looks"];
const ROUND_KEY = "bossylights.review.round";

function load(): Record<string, ScreenReview> {
  try {
    const raw = localStorage.getItem(KEY);
    const all = raw ? (JSON.parse(raw) as Record<string, ScreenReview>) : {};
    if (localStorage.getItem(ROUND_KEY) !== "3") {
      for (const id of REBUILT) {
        const r = all[id];
        if (!r) continue;
        const said = [r.verdict ? `Verdict: ${r.verdict}` : "", r.rating ? `${r.rating}/5` : "", r.comment.trim()].filter(Boolean).join(". ");
        all[id] = { ...r, verdict: null, rating: null, comment: "", previous: said || r.previous };
      }
      localStorage.setItem(ROUND_KEY, "3");
    }
    return all;
  } catch {
    return {};
  }
}

function build() {
  const [reviews, setReviews] = createStore<Record<string, ScreenReview>>(load());

  createEffect(() => {
    const json = JSON.stringify(reviews);
    try {
      localStorage.setItem(KEY, json);
    } catch {
      // storage full or blocked: the review stays in memory until exported
    }
  });

  const ensure = (screen: string) => {
    if (!reviews[screen]) setReviews(screen, empty());
  };

  const get = (screen: string): ScreenReview => reviews[screen] ?? empty();

  function setVerdict(screen: string, verdict: Verdict | null) {
    ensure(screen);
    setReviews(screen, "verdict", verdict);
  }
  function setRating(screen: string, rating: number | null) {
    ensure(screen);
    setReviews(screen, "rating", rating);
  }
  function setComment(screen: string, comment: string) {
    ensure(screen);
    setReviews(screen, "comment", comment);
  }
  function setRegionNote(screen: string, region: string, title: string, note: string) {
    ensure(screen);
    setReviews(screen, "regions", region, { title, note });
  }

  const noteCount = (screen: string) =>
    Object.values(get(screen).regions).filter((r) => r.note.trim()).length;

  // ---- answers to the review questions ----------------------------------
  const ANSWERS_KEY = "bossylights.answers.v1";
  const [answers, setAnswers] = createStore<Record<string, Answer>>(
    (() => {
      try {
        return JSON.parse(localStorage.getItem(ANSWERS_KEY) ?? "{}") as Record<string, Answer>;
      } catch {
        return {};
      }
    })(),
  );
  createEffect(() => {
    const json = JSON.stringify(answers);
    try {
      localStorage.setItem(ANSWERS_KEY, json);
    } catch {
      // kept in memory until exported
    }
  });
  const answerOf = (id: string): Answer => answers[id] ?? { choice: null, text: "" };
  const setChoice = (id: string, choice: Answer["choice"]) => setAnswers(id, { ...answerOf(id), choice });
  const setAnswerText = (id: string, text: string) => setAnswers(id, { ...answerOf(id), text });

  // ---- panel and comment mode ------------------------------------------
  const [panelOpen, setPanelOpen] = createSignal(false);
  const [commentMode, setCommentMode] = createSignal(false);
  /** The region whose note is being edited in the panel. */
  const [activeRegion, setActiveRegion] = createSignal<{ id: string; title: string } | null>(null);

  function openRegion(id: string, title: string) {
    setActiveRegion({ id, title });
    setPanelOpen(true);
  }

  // ---- regions mounted on the current screen ---------------------------
  const [mounted, setMounted] = createStore<Record<string, { id: string; title: string }[]>>({});

  function registerRegion(screen: string, id: string, title: string) {
    setMounted(screen, (list = []) => [...list.filter((r) => r.id !== id), { id, title }]);
    return () => setMounted(screen, (list = []) => list.filter((r) => r.id !== id));
  }

  return {
    answers,
    answerOf,
    setChoice,
    setAnswerText,
    reviews,
    get,
    setVerdict,
    setRating,
    setComment,
    setRegionNote,
    noteCount,
    panelOpen,
    setPanelOpen,
    commentMode,
    setCommentMode,
    activeRegion,
    setActiveRegion,
    openRegion,
    mounted,
    registerRegion,
  };
}

export const review = createRoot(build);

/** The id of the screen a component is rendered in. The shell provides it. */
export const ScreenContext = createContext<string>("unknown");
export const useScreenId = () => useContext(ScreenContext);
