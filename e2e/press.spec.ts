import { expect, test, type Page } from "@playwright/test";

const catalogFrame = (page: Page, key: string) => page.locator(`[data-frame="${key}"]`);
const catalogSent = (page: Page, key: string) => catalogFrame(page, key).locator("[data-sent]");

// A palm, a scroll or the system takes the touch (`pointercancel`): no press, so nothing latches.
test("a cancelled touch on Blackout or a pad is no press: nothing latches", async ({ page }) => {
  await page.goto("/src/catalog/index.html#/top.blackout");
  const blackout = catalogFrame(page, "alone").getByRole("button");
  await expect(blackout).toHaveText("Blackout");

  await blackout.dispatchEvent("pointerdown", { button: 0, pointerId: 1, isPrimary: true });
  await expect(catalogSent(page, "alone")).toHaveText("Blackout on: true");
  await blackout.dispatchEvent("pointercancel", { pointerId: 1, isPrimary: true });

  await expect(
    blackout,
    "pointercancel on Blackout must release the momentary press, not latch Blackout on"
  ).toHaveText("Blackout");
  await expect(catalogSent(page, "alone")).toHaveText("Blackout on: false");

  await page.goto("/src/catalog/index.html#/console.pad");
  const pad = catalogFrame(page, "alone").getByRole("button");
  await expect(pad).toHaveAttribute("data-state", "idle");

  await pad.dispatchEvent("pointerdown", { button: 0, pointerId: 1, isPrimary: true });
  await pad.dispatchEvent("pointercancel", { pointerId: 1, isPrimary: true });

  await expect(
    pad,
    "pointercancel on a look pad must not latch it into manual state"
  ).not.toHaveAttribute("data-state", "manual");
  await expect(pad).toHaveAttribute("data-state", "idle");
  await expect(catalogSent(page, "alone")).toHaveText("EndFlash");
});
