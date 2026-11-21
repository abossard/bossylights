// The UI's connection to the engine, written down as one state machine (docs/design/ui-states.md).
// A calculation: what must happen is returned as effects, done by engine.ts.

export type Connection =
  | "connecting"
  | "connected"
  | "disconnected"
  | "other version"
  | "unreadable reply"
  | "no address"
  | "closed";
export type ConnectionEvent = "matched" | "mismatched" | "opened" | "lost" | "retry" | "garbled" | "close";
/**
 * check: ask the engine its API version. open: a new socket. wait: retry later.
 * shut: close the socket, cancel the retry, and forget the socket so its late close is no event.
 * Effects run in their order: in [shut, wait] the shut cancels the old retry before the new wait.
 */
export type Effect = "check" | "open" | "wait" | "shut";

/** An engine address the window can open, or the text it was given that it cannot. */
export type Address = { url: URL } | { unusable: string };

/** Where the machine starts for this address. */
export function start(address: Address): [Connection, Effect[]] {
  return "url" in address ? ["connecting", ["check"]] : ["no address", []];
}

/** Every transition. A pair that is not a transition changes nothing. */
export function transition(state: Connection, event: ConnectionEvent): [Connection, Effect[]] {
  if (state === "closed") return [state, []];
  if (state === "no address") return event === "close" ? ["closed", []] : [state, []];
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
      case "garbled":
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
      return state === "connecting" || state === "connected" ? ["disconnected", ["wait"]] : [state, []];
    case "retry":
      return state === "disconnected" || state === "unreadable reply" ? ["connecting", ["check"]] : [state, []];
    case "garbled":
      return state === "connected" ? ["unreadable reply", ["shut", "wait"]] : [state, []];
  }
}

/** The event for the engine's answer to `GET /version`: nothing answered, the same version, or not. */
export function versionEvent(answer: string | undefined, mine: number): ConnectionEvent {
  if (answer === undefined) return "lost";
  return answer.trim() === String(mine) ? "matched" : "mismatched";
}

const DEFAULT_ENGINE = "ws://127.0.0.1:7744/api";

/**
 * The engine to talk to: `?engine=` in the page's address, else what the shell told the window, else the default.
 * A WebSocket opens only a ws: or wss: address without a fragment; anything else is unusable.
 */
export function pickEngine(search: string, told: string | undefined): Address {
  const text = new URLSearchParams(search).get("engine") ?? told ?? DEFAULT_ENGINE;
  const url = URL.canParse(text) ? new URL(text) : undefined;
  if (url === undefined || (url.protocol !== "ws:" && url.protocol !== "wss:") || text.includes("#")) return { unusable: text };
  return { url };
}
