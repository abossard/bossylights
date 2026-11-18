import { expect, test } from "@playwright/test";
import { ENGINE } from "../playwright.config";

test("the counter from the engine advances", async ({ page }) => {
  await page.goto(`/?engine=ws://${ENGINE}/state`);
  await expect(page.getByTestId("status")).toHaveText("connected");

  const frame = page.getByTestId("frame");
  await expect(frame).not.toHaveText("0");
  const first = Number(await frame.textContent());
  await page.waitForTimeout(500);
  const second = Number(await frame.textContent());
  // 44 a second: about 22 frames in half a second
  expect(second - first).toBeGreaterThan(10);
});

const states = [
  { engine: `ws://${ENGINE}/state`, status: "connected" },
  { engine: "ws://127.0.0.1:9/state", status: "disconnected" },
];

for (const { engine, status } of states) {
  test(`shows ${status} for ${engine}`, async ({ page }) => {
    await page.goto(`/?engine=${engine}`);
    await expect(page.getByTestId("status")).toHaveText(status);
  });
}
