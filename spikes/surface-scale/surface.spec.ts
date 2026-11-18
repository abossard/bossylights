import { expect, test, type Page } from "@playwright/test";
import { ENGINE } from "./playwright.config";

type Input = { down(x: number, y: number): Promise<void>; move(x: number, y: number): Promise<void>; up(): Promise<void> };

async function mouse(page: Page): Promise<Input> {
  return {
    down: async (x, y) => {
      await page.mouse.move(x, y);
      await page.mouse.down();
    },
    move: (x, y) => page.mouse.move(x, y, { steps: 5 }),
    up: () => page.mouse.up(),
  };
}

async function touch(page: Page): Promise<Input> {
  const cdp = await page.context().newCDPSession(page);
  const send = (type: "touchStart" | "touchMove" | "touchEnd", x = 0, y = 0) =>
    cdp.send("Input.dispatchTouchEvent", { type, touchPoints: type === "touchEnd" ? [] : [{ x, y }] }).then(() => {});
  return { down: (x, y) => send("touchStart", x, y), move: (x, y) => send("touchMove", x, y), up: () => send("touchEnd") };
}

const box = async (page: Page, selector: string) => {
  const b = await page.locator(selector).boundingBox();
  if (!b) throw new Error(`no box for ${selector}`);
  return b;
};

for (const [name, input] of [["mouse", mouse], ["touch", touch]] as const) {
  test.describe(name, () => {
    test.beforeEach(async ({ page }) => {
      await page.goto(`/?engine=ws://${ENGINE}/ws`);
      await expect(page.getByTestId("hud")).toContainText("connected");
    });

    test("drags a widget by its grip", async ({ page }) => {
      const before = await box(page, "[data-testid=widget-5]");
      const grip = await box(page, "[data-testid=widget-5] .grip");
      const i = await input(page);
      await i.down(grip.x + grip.width / 2, grip.y + grip.height / 2);
      await i.move(grip.x + grip.width / 2 + 100, grip.y + grip.height / 2 + 60);
      await i.up();
      const after = await box(page, "[data-testid=widget-5]");
      expect(after.x - before.x).toBeCloseTo(100, 0);
      expect(after.y - before.y).toBeCloseTo(60, 0);
    });

    test("resizes a widget by its corner", async ({ page }) => {
      const before = await box(page, "[data-testid=widget-26]");
      const corner = await box(page, "[data-testid=widget-26] .size");
      const i = await input(page);
      await i.down(corner.x + 6, corner.y + 6);
      await i.move(corner.x + 6 + 40, corner.y + 6 + 30);
      await i.up();
      const after = await box(page, "[data-testid=widget-26]");
      expect(after.width - before.width).toBeCloseTo(40, 0);
      expect(after.height - before.height).toBeCloseTo(30, 0);
    });

    test("a button lights while pressed, from the engine's state", async ({ page }) => {
      const button = page.getByTestId("widget-1");
      const body = await box(page, "[data-testid=widget-1] .body");
      const i = await input(page);
      await i.down(body.x + body.width / 2, body.y + body.height / 2);
      await expect(button).toHaveClass(/\bon\b/);
      await i.up();
      await expect(button).not.toHaveClass(/\bon\b/);
    });

    test("a slider follows the finger, from the engine's state", async ({ page }) => {
      const fill = page.locator("[data-testid=widget-7] .fill");
      const body = await box(page, "[data-testid=widget-7] .body");
      const i = await input(page);
      await i.down(body.x + body.width / 2, body.y + body.height * 0.75);
      await i.move(body.x + body.width / 2, body.y + body.height * 0.25);
      await i.up();
      await expect(fill).toHaveAttribute("style", /height: 7\d(\.\d+)?%/);
    });
  });
}
