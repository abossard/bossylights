// The UI's connection to the engine, written down as one state machine.
// A calculation: what must happen is returned as effects, done by engine.ts.

export type Connection = "connecting" | "connected" | "disconnected" | "closed";
export type ConnectionEvent = "opened" | "lost" | "retry" | "close";
/** open: a new socket. wait: retry later. shut: close the socket, cancel the retry. */
export type Effect = "open" | "wait" | "shut";

export const START: [Connection, Effect[]] = ["connecting", ["open"]];

/** Every transition. A pair that is not a transition changes nothing. */
export function transition(state: Connection, event: ConnectionEvent): [Connection, Effect[]] {
  if (state === "closed") return [state, []];
  switch (event) {
    case "close":
      return ["closed", ["shut"]];
    case "opened":
      return state === "connecting" ? ["connected", []] : [state, []];
    case "lost":
      return state === "disconnected" ? [state, []] : ["disconnected", ["wait"]];
    case "retry":
      return state === "disconnected" ? ["connecting", ["open"]] : [state, []];
  }
}
