// The review questions from docs/design/theory.md and docs/design/modules.md.
// Answers are kept with the review and exported with it.

export interface Question {
  id: string;
  source: "Theory" | "Modules";
  number: number;
  question: string;
  /** Option A, the author's lean. Empty for open questions. */
  a: string;
  b: string;
  /** Set once the owner has decided: shown in place of the options. */
  decided?: string;
}

const q = (source: Question["source"], number: number, question: string, a = "", b = "", decided?: string): Question => ({
  id: `${source.toLowerCase()}-${number}`,
  source,
  number,
  question,
  a,
  b,
  decided,
});

export const QUESTIONS: Question[] = [
  q("Theory", 1, "Who wins?", "Top layer wins for everything", "Intensity adds up, so two washes are brighter", "Manual beats show beats autopilot. Among equals the latest wins. Brightness can be set to \"highest wins\"."),
  q("Theory", 2, "How many layers?", "Three, fixed", "The user can add layers", "None to manage. The layer idea was dropped: only steps, sequences and patterns play."),
  q("Theory", 3, "What does recording store?", "Commands: editable, follows rig changes", "DMX values: exact, but frozen"),
  q("Theory", 4, "Where do raw channels live?", "Only on the DMX screen", "Everywhere"),
  q("Theory", 5, "DJ software and audio disagree on the beat", "The feed you picked wins", "The most confident feed wins"),
  q("Theory", 6, "What is Song Director?", "An editor for a show whose blocks follow song sections", "Its own thing"),
  q("Theory", 7, "Anything you rely on in QLC+ that these nine ideas cannot say?"),
  q("Modules", 1, "Is the Engine with three operations believable?", "Yes, commands are data", "No, it hides a giant \"do anything\"", "It is called Engine and accepts a listed set of commands"),
  q("Modules", 2, "Can you play something that is not saved?", "No, everything playable lives in Project", "Yes, scratch sources exist"),
  q("Modules", 3, "Where does controller mapping live?", "In Surface, next to the widgets", "In In/Out, like QLC+ profiles"),
  q("Modules", 4, "Music and Autopilot", "Two modules: knowing vs deciding", "One module"),
  q("Modules", 5, "Which module will be hardest to keep narrow?"),
];
