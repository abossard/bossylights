// The only file that talks to the engine: sockets and timers live here.
// What happens next is decided by the state machine in connection.ts; what the
// engine reported is kept by picture.ts.
import { createSignal, type Accessor } from "solid-js";
import { pickEngine, start, transition, versionEvent, type Address, type Connection, type ConnectionEvent, type Effect } from "./connection";
import { API_VERSION, type Request } from "./engine-api";
import { decode, receive, type Picture } from "./picture";

export interface Engine {
  picture: Accessor<Picture | undefined>;
  connection: Accessor<Connection>;
  close: () => void;
}

const RETRY_MS = 1000;

/** The engine's API. `?engine=ws://host:port/api` picks another engine; the desktop shell names its own. */
export function engineAddress(): Address {
  return pickEngine(location.search, (window as { __BOSSYLIGHTS_ENGINE__?: string }).__BOSSYLIGHTS_ENGINE__);
}

/** Connects to the engine, subscribes to its state and keeps reconnecting until closed. */
export function connectEngine(address: Address): Engine {
  const [first, startEffects] = start(address);
  const [picture, setPicture] = createSignal<Picture>();
  const [connection, setConnection] = createSignal<Connection>(first);
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
    if (!("url" in address)) return;
    const url = address.url;
    switch (effect) {
      case "check": {
        const version = new URL("/version", url.href.replace(/^ws/, "http"));
        void fetch(version).then(
          (response) => (response.ok ? response.text() : `not ok: ${response.status}`),
          () => undefined,
        ).then((answer) => send(versionEvent(answer, API_VERSION)));
        return;
      }
      case "open": {
        const opened = new WebSocket(url);
        socket = opened;
        opened.onopen = () => {
          send("opened");
          request("Subscribe");
        };
        opened.onclose = () => send("lost");
        opened.onmessage = (message: MessageEvent<string>) => {
          const reply = decode(message.data);
          if (reply === undefined) send("garbled");
          else setPicture((before) => receive(before, reply));
        };
        return;
      }
      case "wait":
        retry = setTimeout(() => send("retry"), RETRY_MS);
        return;
      case "shut":
        clearTimeout(retry);
        if (socket !== undefined) {
          socket.onopen = socket.onclose = socket.onmessage = null;
          socket.close();
          socket = undefined;
        }
        return;
    }
  };

  startEffects.forEach(perform);
  return { picture, connection, close: () => send("close") };
}
