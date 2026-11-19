import { expect, test } from "@playwright/test";
import { createSocket } from "node:dgram";
import { createServer } from "node:http";
import { ENGINE } from "./ports";

test("the counter from the engine advances", async ({ page }) => {
  await page.goto(`/?engine=ws://${ENGINE}/api`);
  await expect(page.getByTestId("status")).toHaveText("connected");

  const frame = page.getByTestId("frame");
  await expect(frame).not.toHaveText("0");
  const first = Number(await frame.textContent());
  await expect.poll(async () => Number(await frame.textContent())).toBeGreaterThan(first);
});

test("the debug page shows the output thread's priority", async ({ page }) => {
  await page.goto(`/?engine=ws://${ENGINE}/api`);
  await expect(page.getByTestId("priority")).toHaveText(/^output thread: (real-time|best effort: .+)$/);
});

test("a fader sets channel 1 through the API and shows the engine's level", async ({ page }) => {
  await page.goto(`/?engine=ws://${ENGINE}/api`);
  await expect(page.getByTestId("status")).toHaveText("connected");
  await page.getByTestId("fader-1").fill("200");
  await expect(page.getByTestId("level-1")).toHaveText("200");
});

test("an output added through the API sends a fader's level as DDP and shows its counters", async ({ page }) => {
  const udp = createSocket("udp4");
  const packets: Buffer[] = [];
  udp.on("message", (packet) => packets.push(packet));
  await new Promise<void>((bound) => udp.bind(0, "127.0.0.1", bound));
  try {
    const address = `127.0.0.1:${udp.address().port}`;
    await page.goto(`/?engine=ws://${ENGINE}/api`);
    await expect(page.getByTestId("status")).toHaveText("connected");
    await page.evaluate(
      async ([url, address]) => {
        const socket = new WebSocket(url);
        await new Promise((opened) => (socket.onopen = opened));
        const kind = { Ddp: { address, color: "Rgb" } };
        const command = { SetOutput: { output: 16, kind, universes: { first: 1, count: 1 }, rate: "30" } };
        socket.send(JSON.stringify({ Send: { command } }));
        await new Promise((answered) => (socket.onmessage = answered));
        socket.close();
      },
      [`ws://${ENGINE}/api`, address],
    );
    await expect(page.getByTestId("outputs")).toContainText(/16: universes 1 to 1, 30 a second, Running, accepted [1-9]/);
    await page.getByTestId("fader-2").fill("123");
    // Header: version and push, sequence, RGB, destination; then channel 2 of universe 1.
    await expect.poll(() => packets.at(-1)?.[10 + 1]).toBe(123);
    expect([...packets.at(-1)!.subarray(0, 4)]).toEqual([0x41, packets.at(-1)![1], 0x0b, 1]);
  } finally {
    await page.evaluate(
      async (url) => {
        const socket = new WebSocket(url);
        await new Promise((opened) => (socket.onopen = opened));
        socket.send(JSON.stringify({ Send: { command: { RemoveOutput: { output: 16 } } } }));
        await new Promise((answered) => (socket.onmessage = answered));
        socket.close();
      },
      `ws://${ENGINE}/api`,
    );
    udp.close();
  }
});

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
