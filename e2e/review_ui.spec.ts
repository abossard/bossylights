import { expect, test, type Page } from "@playwright/test";
import { ENGINE } from "./ports";

const catalogFrame = (page: Page, key: string) => page.locator(`[data-frame="${key}"]`);
const catalogSent = (page: Page, key: string) => catalogFrame(page, key).locator("[data-sent]");

/**
 * Claim: `decode()` in `ui/src/picture.ts` accepts arbitrary JSON strings, unknown variant objects, and malformed `Picture` payloads as valid `Reply` messages instead of returning `undefined`, so the UI stays `"connected"` (and throws in `where()`) instead of entering `"unreadable reply"`.
 * Source: `docs/design/ui-states.md` ("unreadable reply" connection state) and `ui/src/picture.ts` ("A garbled reply returns `undefined` so the caller closes the connection and says why").
 * Severity: medium.
 * Show impact: When a proxy, truncated frame, or mismatched engine sends an unknown or malformed reply variant, the desk falsely shows `"connected"` over corrupted or uncaught-exception UI state instead of surfacing `"unreadable reply"`.
 */
test("finding_ui_01_picture_decode_accepts_garbled_replies", async ({ page }) => {
  const garbledPayloads = [
    '"BogusVariant"',
    '{"UnknownVariant":{"x":1}}',
    '{"Picture":{"state":{"outputs":[{}]},"project":{"outputs":[{"Kind":{}}]},"library":"Empty","universes":[[]]}}',
  ];

  for (const payload of garbledPayloads) {
    await page.unrouteAll({ behavior: "ignoreErrors" });
    await page.routeWebSocket(`ws://${ENGINE}/api`, (ws) => {
      ws.onMessage(() => {
        ws.send(payload);
      });
    });

    await page.goto(`/?engine=ws://${ENGINE}/api`);
    await expect(
      page.getByTestId("status"),
      `payload ${payload} must put the UI into "unreadable reply"`
    ).toHaveText("unreadable reply", { timeout: 2_000 });
  }
});

/**
 * Claim: `Pad` and `Button` wire `onPointerCancel` directly to `pointerUp` / `up`, so an OS gesture cancellation (`pointercancel` from scroll, palm rejection, or system overlay) within 350 ms is treated as a deliberate tap (`upShort`) and latches Blackout or a look pad ON instead of cancelling the press.
 * Source: `docs/design/kit.md` ("Hold and latch" gesture contract) and W3C Pointer Events (`pointercancel` aborts an active pointer interaction and must not commit a tap/latch).
 * Severity: high.
 * Show impact: Brushing the touchscreen with a palm or triggering an OS edge/scroll gesture over Blackout or a look pad latches the stage into Blackout or fires/latches a look unintentionally.
 */
test("finding_ui_02_pointer_cancel_latches_pad_and_blackout", async ({ page }) => {
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
  await expect(pad).toHaveAttribute("data-state", "off");

  await pad.dispatchEvent("pointerdown", { button: 0, pointerId: 1, isPrimary: true });
  await pad.dispatchEvent("pointercancel", { pointerId: 1, isPrimary: true });

  await expect(
    pad,
    "pointercancel on a look pad must not latch it into manual state"
  ).not.toHaveAttribute("data-state", "manual");
});

/**
 * Claim: `Fader` tracks pointer drag with a single boolean `down` without checking `e.pointerId`, so a second finger touching and lifting (`pointerId: 2`) clears `down` while the first finger (`pointerId: 1`) is still dragging the fader.
 * Source: `docs/design/kit.md` (`Fader` continuous pointer tracking on touch/multi-touch surfaces).
 * Severity: medium.
 * Show impact: On a multi-touch desk screen, resting or lifting a second finger while riding Grand Master or a channel fader freezes the active fader drag mid-fade until the operator lifts and grabs it again.
 */
test("finding_ui_03_second_finger_aborts_active_fader_drag", async ({ page }) => {
  await page.goto("/src/catalog/index.html#/console.fader");
  const fader = catalogFrame(page, "alone").getByRole("slider");
  const box = await fader.boundingBox();
  if (!box) throw new Error("fader has no bounding box");

  const midX = box.x + box.width / 2;
  const bottomY = box.y + box.height - 4;
  const topY = box.y + 4;

  // Finger 1 grabs the fader near the bottom and drags halfway up.
  await fader.dispatchEvent("pointerdown", {
    button: 0,
    pointerId: 1,
    clientX: midX,
    clientY: bottomY,
  });
  await fader.dispatchEvent("pointermove", {
    pointerId: 1,
    clientX: midX,
    clientY: (bottomY + topY) / 2,
  });

  // Finger 2 briefly brushes and lifts while finger 1 stays down.
  await fader.dispatchEvent("pointerdown", {
    button: 0,
    pointerId: 2,
    clientX: midX,
    clientY: (bottomY + topY) / 2,
  });
  await fader.dispatchEvent("pointerup", {
    pointerId: 2,
    clientX: midX,
    clientY: (bottomY + topY) / 2,
  });

  // Finger 1 continues dragging all the way to 100% at the top.
  await fader.dispatchEvent("pointermove", {
    pointerId: 1,
    clientX: midX,
    clientY: topY,
  });

  await expect(
    fader,
    "finger 1 is still dragging to the top; finger 2 lifting must not abort finger 1's fader drag"
  ).toHaveAttribute("aria-valuenow", "100");
});
