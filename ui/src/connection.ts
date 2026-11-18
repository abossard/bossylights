// The UI's connection to the engine, written down as one state machine.
// A calculation: what must happen is returned as effects, done by engine.ts.

export type Connection = "connecting" | "connected" | "disconnected" | "other version" | "closed";
export type ConnectionEvent = "matched" | "mismatched" | "opened" | "lost" | "retry" | "close";
/** check: ask the engine its API version. open: a new socket. wait: retry later. shut: close the socket, cancel the retry. */
export type Effect = "check" | "open" | "wait" | "shut";

export const START: [Connection, Effect[]] = ["connecting", ["check"]];

/** Every transition. A pair that is not a transition changes nothing. */
export function transition(state: Connection, event: ConnectionEvent): [Connection, Effect[]] {
  if (state === "closed") return [state, []];
  if (event === "close") return ["closed", ["shut"]];
  // An engine of another version is not talked to, but asked again: the right one may be started later.
  if (state === "other version") {
    switch (event) {
      case "matched":
        return ["connecting", ["open"]];
      case "mismatched":
      case "lost":
        return [state, ["wait"]];
      case "retry":
        return [state, ["check"]];
      case "opened":
        return [state, []];
    }
  }
  switch (event) {
    case "matched":
      return state === "connecting" ? [state, ["open"]] : [state, []];
    case "mismatched":
      return state === "connecting" ? ["other version", ["wait"]] : [state, []];
    case "opened":
      return state === "connecting" ? ["connected", []] : [state, []];
    case "lost":
      return state === "disconnected" ? [state, []] : ["disconnected", ["wait"]];
    case "retry":
      return state === "disconnected" ? ["connecting", ["check"]] : [state, []];
  }
}

/** The event for the engine's answer to `GET /version`: nothing answered, the same version, or not. */
export function versionEvent(answer: string | undefined, mine: number): ConnectionEvent {
  if (answer === undefined) return "lost";
  return answer.trim() === String(mine) ? "matched" : "mismatched";
}

/** Why a command sent in this state goes nowhere, or undefined when it goes out. */
export function notSent(state: Connection): string | undefined {
  return state === "connected" ? undefined : `not sent: the engine is ${state}`;
}

const DEFAULT_ENGINE = "ws://127.0.0.1:7744/api";

/** The engine to talk to: `?engine=` in the page's address, else what the shell told the window, else the default. */
export function pickEngine(search: string, told: string | undefined): string {
  return new URLSearchParams(search).get("engine") ?? told ?? DEFAULT_ENGINE;
}
