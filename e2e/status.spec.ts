import { expect, test } from "@playwright/test";
import { createSocket } from "node:dgram";
import { createServer } from "node:http";
import { ENGINE } from "./ports";

/** Sends one command over the engine's API, as any client does, and returns its answer. */
async function send(command: object): Promise<unknown> {
  const socket = new WebSocket(`ws://${ENGINE}/api`);
  await new Promise((opened) => (socket.onopen = opened));
  const answer = new Promise<MessageEvent>((answered) => (socket.onmessage = answered));
  socket.send(JSON.stringify({ Send: { command } }));
  const reply = JSON.parse(String((await answer).data));
  socket.close();
  return reply;
}

// [channel of universe 1, value]. DDP (3waylabs): a 10 byte header, then the data from its first channel.
const levels = [
  [1, 200],
  [2, 123],
];

for (const [address, value] of levels) {
  test(`channel ${address} set through the API goes out as DDP and the page says where the output sends`, async ({ page }) => {
    const udp = createSocket("udp4");
    const packets: Buffer[] = [];
    udp.on("message", (packet) => packets.push(packet));
    await new Promise<void>((bound) => udp.bind(0, "127.0.0.1", bound));
    const output = 17 - address;
    try {
      const to = `127.0.0.1:${udp.address().port}`;
      await page.goto(`/?engine=ws://${ENGINE}/api`);
      await expect(page.getByTestId("status")).toHaveText("connected");
      const kind = { Ddp: { address: to, color: "Rgb" } };
      expect(await send({ SetOutput: { output, kind, universes: { first: 1, count: 1 }, rate: "30" } })).toBe("Accepted");
      await expect(page.getByTestId("outputs")).toContainText(`DDP to ${to}, running, 30 a second`);
      const channel = { universe: 1, address };
      // First 0, then the value: a level left by another test cannot pass for this one.
      for (const level of [0, value]) {
        expect(await send({ SetChannel: { channel, value: level } })).toBe("Accepted");
        await expect.poll(() => packets.at(-1)?.[10 + address - 1]).toBe(level);
      }
      // Header: version and push, sequence, RGB, destination.
      expect([...packets.at(-1)!.subarray(0, 4)]).toEqual([0x41, packets.at(-1)![1], 0x0b, 1]);
    } finally {
      await send({ ReleaseChannel: { channel: { universe: 1, address } } });
      await send({ RemoveOutput: { output } });
      udp.close();
    }
  });
}

const states = [
  { engine: `ws://${ENGINE}/api`, status: "connected" },
  { engine: "ws://127.0.0.1:9/api", status: "disconnected" },
];

for (const { engine, status } of states) {
  test(`shows ${status} for ${engine}`, async ({ page }) => {
    await page.goto(`/?engine=${engine}`);
    await expect(page.getByTestId("status")).toHaveText(status);
  });
}

test("shows other version for an engine that speaks another API version", async ({ page }) => {
  const other = createServer((ask, answer) => {
    answer.setHeader("access-control-allow-origin", "*");
    if (ask.url === "/version") answer.end("0");
    else answer.writeHead(404).end();
  });
  await new Promise<void>((listening) => other.listen(0, "127.0.0.1", listening));
  try {
    const address = other.address();
    const port = typeof address === "object" && address ? address.port : 0;
    await page.goto(`/?engine=ws://127.0.0.1:${port}/api`);
    await expect(page.getByTestId("status")).toHaveText("other version");
  } finally {
    await new Promise((closed) => other.close(closed));
  }
});
