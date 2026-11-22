// The engine's API as the tests speak it: typed by the generated engine-api.ts, so a test
// cannot send a message the engine does not know.
import type { Command, Reply, Request } from "../ui/src/engine-api.ts";

type Picture = Extract<Reply, { Picture: unknown }>["Picture"];

/** The engine's first answer to a request that is not a `Changed`, as any client gets it. */
export async function request(address: string, message: Request): Promise<Reply> {
  const socket = new WebSocket(`ws://${address}/api`);
  await new Promise((opened, failed) => {
    socket.onopen = opened;
    socket.onerror = failed;
  });
  const reply = new Promise<Reply>((answered) => {
    socket.onmessage = (event) => {
      const data: Reply = JSON.parse(String(event.data));
      if (typeof data === "string" || !("Changed" in data)) answered(data);
    };
  });
  socket.send(JSON.stringify(message));
  const answer = await reply;
  socket.close();
  return answer;
}

/** Sends one command and answers the engine's reply to it. */
export const send = (address: string, command: Command): Promise<Reply> => request(address, { Send: { command } });

/** What the engine answers a `Query`: its picture. */
export async function query(address: string): Promise<Picture> {
  const reply = await request(address, "Query");
  if (typeof reply === "string" || !("Picture" in reply)) throw new Error(`a Query answered ${JSON.stringify(reply)}`);
  return reply.Picture;
}
