// The only file that talks to the engine: sockets and timers live here.
// What happens next is decided by the state machine in connection.ts.
import { createSignal, type Accessor } from "solid-js";
import { START, transition, type Connection, type ConnectionEvent, type Effect } from "./connection";
import type { Priority, State } from "./engine-api";

export interface Engine {
  frame: Accessor<number | undefined>;
  priority: Accessor<Priority | undefined>;
  connection: Accessor<Connection>;
  close: () => void;
}

const DEFAULT_URL = "ws://127.0.0.1:7744/state";
const RETRY_MS = 1000;

/** The engine's state stream. `?engine=ws://host:port/state` picks another engine. */
export function engineUrl(): string {
  return new URLSearchParams(location.search).get("engine") ?? DEFAULT_URL;
}

/** Connects to the engine's state stream and keeps reconnecting until closed. */
export function connectEngine(url: string): Engine {
  const [start, startEffects] = START;
  const [frame, setFrame] = createSignal<number>();
  const [priority, setPriority] = createSignal<Priority>();
  const [connection, setConnection] = createSignal<Connection>(start);
  let socket: WebSocket | undefined;
  let retry: ReturnType<typeof setTimeout> | undefined;

  const send = (event: ConnectionEvent) => {
    const [next, effects] = transition(connection(), event);
    setConnection(next);
    effects.forEach(perform);
  };

  const perform = (effect: Effect) => {
    switch (effect) {
      case "open":
        socket = new WebSocket(url);
        socket.onopen = () => send("opened");
        socket.onclose = () => send("lost");
        socket.onmessage = (message: MessageEvent<string>) => {
          const state = JSON.parse(message.data) as State;
          setFrame(state.frame);
          setPriority(state.priority);
        };
        return;
      case "wait":
        retry = setTimeout(() => send("retry"), RETRY_MS);
        return;
      case "shut":
        clearTimeout(retry);
        socket?.close();
        return;
    }
  };

  startEffects.forEach(perform);
  return { frame, priority, connection, close: () => send("close") };
}
