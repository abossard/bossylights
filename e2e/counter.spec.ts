import { expect, test } from "@playwright/test";
import { ENGINE } from "../playwright.config";

test("the counter from the engine advances", async ({ page }) => {
  await page.goto(`/?engine=ws://${ENGINE}/api`);
  await expect(page.getByTestId("status")).toHaveText("connected");

  const frame = page.getByTestId("frame");
  await expect(frame).not.toHaveText("0");
  const first = Number(await frame.textContent());
  await page.waitForTimeout(500);
  const second = Number(await frame.textContent());
  // 44 a second: about 22 frames in half a second
  expect(second - first).toBeGreaterThan(10);
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
