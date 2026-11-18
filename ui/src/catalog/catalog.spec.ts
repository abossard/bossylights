import { expect, test, type Page } from "@playwright/test";
import { SITUATIONS } from "../controls/situations.ts";
import { CONTROLS } from "../controls/table.ts";

// The headless run of the catalog: every page mounts, and a gesture shows the command it sends.
// It checks what is sent, never how fast.

const frame = (page: Page, key: string) => page.locator(`[data-frame="${key}"]`);
const sent = (page: Page, key: string) => frame(page, key).locator("[data-sent]");

test("the index lists every control and the search narrows it", async ({ page }) => {
  await page.goto("/");
  await expect(page.locator("[data-link]")).toHaveCount(CONTROLS.length);
  await page.getByLabel("Search").fill("blackout");
  await expect(page.locator("[data-link]")).toHaveCount(1);
  await expect(page.locator("[data-count]")).toHaveText(`1 of ${CONTROLS.length} controls`);
  await page.locator('[data-link="top.blackout"]').click();
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Blackout");
});

test("every control has its page: the element alone, a frame per situation and per state, no error", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  await page.goto("/");
  for (const control of CONTROLS) {
    await page.goto(`/#/${control.id}`);
    await expect(page.locator(`[data-page="${control.id}"] h1`)).toHaveText(control.name);
    await expect(page.locator("[data-frame]")).toHaveCount(1 + SITUATIONS.length + control.states.length);
    await expect(frame(page, "alone").locator(".c-stage > *")).toHaveCount(1);
  }
  expect(errors).toEqual([]);
});

// [control, frame, the control in the frame by role and name, the command shown]
const presses = [
  ["top.blackout", "alone", "button", "Blackout", "Blackout on: true"],
  ["top.live", "alone", "switch", /LIVE is off/, "SetLive on: true"],
  ["top.tap", "alone", "button", "Tap", "TapTempo"],
  ["top.tap", "learn", "button", "Tap", "StartLearn top.tap"],
  ["console.pad", "alone", "button", "Warm wash", "StartLook"],
  ["console.running.release-all", "holding", "button", "Release all", "ReleaseAll"],
  ["console.speed", "alone", "radio", "double", "SetSpeed double"],
  ["songs.section.remove-look", "alone", "button", "Take Warm wash out", "RemoveSectionLook"],
  ["menubar.stop-engine", "window-closed", "menuitem", /Stop engine/, "StopEngine"],
] as const;
for (const [id, key, role, name, command] of presses) {
  test(`a press shows what is sent: ${id} in ${key}`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    await expect(sent(page, key)).toHaveText("Nothing sent yet");
    await frame(page, key).getByRole(role, { name }).click();
    await expect(sent(page, key)).toHaveText(command);
    await expect(page.locator("[data-log] li").first()).toContainText(command);
  });
}

// [control, frame, the reason shown on the control]
const offs = [
  ["console.running.release-all", "alone", "Nothing held"],
  ["menubar.stop-engine", "live-on", "LIVE is on"],
  ["top.learn", "lock", "Locked while LIVE is on"],
  ["console.pad", "no-engine", "Engine not connected"],
] as const;
for (const [id, key, reason] of offs) {
  test(`a control that is off says why and sends nothing: ${id} in ${key}`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    const control = frame(page, key).locator(".c-stage button").first();
    await expect(control).toBeDisabled();
    await expect(control).toContainText(reason);
    await control.click({ force: true });
    await expect(sent(page, key)).toHaveText("Nothing sent yet");
  });
}

test("a control that is not there leaves an empty frame with the reason", async ({ page }) => {
  await page.goto("/#/top.blackout");
  await expect(frame(page, "window-closed").locator(".c-stage")).toHaveText("Not there");
  await expect(frame(page, "window-closed")).toContainText("The window is closed");
});

test("the pad: a press latches it, a press on a running pad switches it off and it stays held", async ({ page }) => {
  await page.goto("/#/console.pad");
  const pad = frame(page, "alone").getByRole("button");
  await pad.click();
  await expect(pad).toHaveAttribute("data-state", "manual");
  await expect(sent(page, "alone")).toHaveText("StartLook");
  await pad.click();
  await expect(pad).toHaveAttribute("data-state", "kept");
  await expect(sent(page, "alone")).toHaveText("StopLook");
});

test("Blackout: Space holds it, the button latches it", async ({ page }) => {
  await page.goto("/#/top.blackout");
  const button = frame(page, "alone").getByRole("button");
  await button.focus();
  await page.keyboard.down("Space");
  await expect(button).toHaveText(/Blackout on/);
  await expect(sent(page, "alone")).toHaveText("Blackout on: true");
  await page.keyboard.up("Space");
  await expect(button).toHaveText("Blackout");
  await expect(sent(page, "alone")).toHaveText("Blackout on: false");
  await page.keyboard.press("Enter");
  await expect(button).toHaveText(/Blackout on/);
});

test("a fader follows the keyboard and sends its level", async ({ page }) => {
  await page.goto("/#/console.fader");
  const fader = frame(page, "alone").getByRole("slider");
  await fader.focus();
  await page.keyboard.press("End");
  await expect(fader).toHaveAttribute("aria-valuenow", "100");
  await expect(sent(page, "alone")).toHaveText("SetMaster 100");
});

test("a light dragged in the universe grid sends its new address once, when dropped", async ({ page }) => {
  await page.goto("/#/patch.grid.light");
  const light = frame(page, "alone").getByRole("button");
  const box = await light.boundingBox();
  if (!box) throw new Error("the light is not on screen");
  await page.mouse.move(box.x + 10, box.y + 10);
  await page.mouse.down();
  await page.mouse.move(box.x + 60, box.y + 10, { steps: 4 });
  await expect(light).toHaveAttribute("data-state", "lifted");
  await expect(sent(page, "alone")).toHaveText("Nothing sent yet");
  await page.mouse.up();
  await expect(sent(page, "alone")).toHaveText("SetAddress where it landed");
  await expect(light).toHaveAttribute("data-state", "plain");
});

test("a rating and a comment stay, show in the index and come out in the export", async ({ page }) => {
  await page.goto("/#/top.tap");
  await page.getByRole("button", { name: "4 of 5" }).click();
  await page.getByLabel("What works, what does not?").fill("Feels right");
  await page.reload();
  await expect(page.getByRole("button", { name: "4 of 5" })).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByLabel("What works, what does not?")).toHaveValue("Feels right");
  await expect(page.locator('[data-link="top.tap"]')).toContainText("4");
  await page.goto("/#/");
  await expect(page.locator("[data-rated]")).toHaveText("1 rated");
  await expect(page.locator("[data-export]")).toHaveValue(/\| Tap \| `top\.tap` \| Top bar \| 4 of 5 \| Feels right \|/);
});
