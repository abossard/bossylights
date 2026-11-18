// The only file that talks to the engine. Everything else reads these signals.
import { createSignal, type Accessor } from "solid-js";

export type Status = "connecting" | "connected" | "disconnected";

export interface Engine {
  frame: Accessor<number | undefined>;
  status: Accessor<Status>;
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
  const [frame, setFrame] = createSignal<number>();
  const [status, setStatus] = createSignal<Status>("connecting");
  let socket: WebSocket | undefined;
  let retry: ReturnType<typeof setTimeout> | undefined;
  let closed = false;

  const open = () => {
    socket = new WebSocket(url);
    socket.onopen = () => setStatus("connected");
    socket.onmessage = (event) => setFrame(JSON.parse(event.data).frame);
    socket.onclose = () => {
      setStatus("disconnected");
      if (!closed) retry = setTimeout(open, RETRY_MS);
    };
  };
  open();

  return {
    frame,
    status,
    close: () => {
      closed = true;
      clearTimeout(retry);
      socket?.close();
    },
  };
}
