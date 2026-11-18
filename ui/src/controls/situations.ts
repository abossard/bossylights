// The situations every control is checked in: the columns of the behaviour table (issue #102).
// Each one is the usual state plus one fact. The usual state: LIVE off, the engine connected,
// lights patched, nothing held, no song, the autopilot off, every output running.

export const SITUATIONS = [
  { id: "live-off", name: "LIVE off", fact: "Preparing. Lights follow the app, nothing is recorded." },
  { id: "live-on", name: "LIVE on", fact: "The night is running. The lock has its own column." },
  { id: "lock", name: "Lock on", fact: "LIVE is on and the lock is in effect: the Console layout cannot change." },
  { id: "recording", name: "Recording on", fact: "LIVE is on and a song plays: what you play is written into its Recorded row." },
  { id: "autopilot", name: "Autopilot on", fact: "The autopilot plays its picks." },
  { id: "holding", name: "Something held", fact: "You hold at least one thing by hand, until Release." },
  { id: "no-engine", name: "Engine not connected", fact: "The window has lost the engine. The lights keep their last frame." },
  { id: "empty", name: "Nothing patched", fact: "An empty project: no lights, no looks, no songs." },
  { id: "edit-surface", name: "Edit surface", fact: "The Console layout is being arranged." },
  { id: "learn", name: "Learn mode", fact: "Learn waits for you to pick a control on screen." },
  { id: "song", name: "A song plays", fact: "A deck plays an analysed song." },
  { id: "no-song", name: "No song plays", fact: "Both decks are stopped or empty." },
  { id: "output-lost", name: "An output lost", fact: "One output device is gone and reconnecting. The others run." },
] as const;

export type SituationId = (typeof SITUATIONS)[number]["id"];

/** What a control does in one situation: one cell of the behaviour table. */
export type Cell =
  | { is: "works"; says: string }
  /** It is there and cannot be pressed. `says` is the reason, shown on the control. */
  | { is: "off"; says: string }
  /** It is not on screen. `says` is why. */
  | { is: "gone"; says: string };

export const works = (says: string): Cell => ({ is: "works", says });
export const off = (says: string): Cell => ({ is: "off", says });
export const gone = (says: string): Cell => ({ is: "gone", says });

/** The six gestures the table answers for every control. */
export const GESTURES = ["press", "hold", "double press", "drag", "keyboard", "touch"] as const;
export type Gesture = (typeof GESTURES)[number];
