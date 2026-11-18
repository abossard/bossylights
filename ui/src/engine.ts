// The only file that talks to the engine: sockets and timers live here.
// What happens next is decided by the state machine in connection.ts; what the
// engine reported is kept by picture.ts.
import { createSignal, type Accessor } from "solid-js";
import { START, transition, type Connection, type ConnectionEvent, type Effect } from "./connection";
import type { Command, Refusal, Reply, Request } from "./engine-api";
import { receive, type Picture } from "./picture";

export interface Engine {
  picture: Accessor<Picture | undefined>;
  /** Why the last command was refused, until one is accepted. */
  refusal: Accessor<Refusal | undefined>;
  connection: Accessor<Connection>;
  send: (command: Command) => void;
  close: () => void;
}

const DEFAULT_URL = "ws://127.0.0.1:7744/api";
const RETRY_MS = 1000;

/** The engine's API. `?engine=ws://host:port/api` picks another engine. */
export function engineUrl(): string {
  return new URLSearchParams(location.search).get("engine") ?? DEFAULT_URL;
}

/** Connects to the engine, subscribes to its state and keeps reconnecting until closed. */
export function connectEngine(url: string): Engine {
  const [start, startEffects] = START;
  const [picture, setPicture] = createSignal<Picture>();
  const [refusal, setRefusal] = createSignal<Refusal>();
  const [connection, setConnection] = createSignal<Connection>(start);
  let socket: WebSocket | undefined;
  let retry: ReturnType<typeof setTimeout> | undefined;

  const request = (message: Request) => {
    if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify(message));
  };

  const send = (event: ConnectionEvent) => {
    const [next, effects] = transition(connection(), event);
    setConnection(next);
    effects.forEach(perform);
  };

  const perform = (effect: Effect) => {
    switch (effect) {
      case "open":
        socket = new WebSocket(url);
        socket.onopen = () => {
          send("opened");
          request("Subscribe");
        };
        socket.onclose = () => send("lost");
        socket.onmessage = (message: MessageEvent<string>) => {
          const reply = JSON.parse(message.data) as Reply;
          setPicture((before) => receive(before, reply));
          if (reply === "Accepted") setRefusal(undefined);
          else if ("Refused" in reply) setRefusal(reply.Refused.reason);
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
  return {
    picture,
    refusal,
    connection,
    send: (command) => request({ Send: { command } }),
    close: () => send("close"),
  };
}
