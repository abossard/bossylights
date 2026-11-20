import { expect, test, type Page } from "@playwright/test";
import { SITUATIONS } from "../controls/situations.ts";
import { PRIMITIVES } from "../controls/spec.ts";
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

const KIT = Object.keys(PRIMITIVES);

test("the index starts with the primitives, and where a control lives is a tag that narrows it", async ({ page }) => {
  await page.goto("/");
  await expect(page.locator("[data-kit-link]")).toHaveCount(KIT.length);
  await expect(page.locator(".c-list section").first()).toContainText("Primitives");
  await expect(page.locator('[data-link="top.blackout"]')).toContainText("Top bar");
  // The picker of the index is the kit's own list.
  await page.getByRole("button", { name: /Where it lives/ }).click();
  await page.getByRole("option", { name: "Menu bar" }).click();
  await expect(page.locator("[data-link]")).toHaveCount(CONTROLS.filter((c) => c.screen === "Menu bar").length);
  await expect(page.locator("[data-kit-link]")).toHaveCount(KIT.length);
});

test("every primitive has its page: kinds, states and colours side by side, the controls drawn with it, no error", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  await page.goto("/");
  for (const primitive of KIT) {
    await page.locator(`[data-kit-link="${primitive}"]`).click();
    await expect(page.locator(`[data-kit-page="${primitive}"] h1`)).toHaveText(primitive);
    await expect(page.locator("[data-specimen]").first()).toBeVisible();
    await expect(page.locator("[data-specimen] .c-stage:empty")).toHaveCount(0);
    const drawn = CONTROLS.filter((c) => c.kit === primitive || c.kit.startsWith(`${primitive}.`)).length;
    await expect(page.locator(`[data-kit-page="${primitive}"] tbody a`)).toHaveCount(drawn);
  }
  expect(errors).toEqual([]);
});

test("a control names its primitive and leads to it", async ({ page }) => {
  await page.goto("/#/top.indicator.autopilot");
  await page.locator("[data-kit-of]").click();
  await expect(page.locator('[data-kit-page="Button"] h1')).toHaveText("Button");
});

test("a list is the kit's own: it opens, arrows move, Enter picks, Escape changes nothing", async ({ page }) => {
  await page.goto("/#/console.autopilot.style");
  const list = frame(page, "alone").locator(".k-list-button");
  await expect(list).toContainText("House");
  await list.click();
  await expect(frame(page, "alone").getByRole("option")).toHaveCount(3);
  await page.keyboard.press("Escape");
  await expect(frame(page, "alone").getByRole("option")).toHaveCount(0);
  await expect(sent(page, "alone")).toHaveText("Nothing sent yet");
  await list.press("ArrowDown");
  await list.press("ArrowDown");
  await list.press("Enter");
  await expect(list).toContainText("Techno");
  await expect(sent(page, "alone")).toContainText("Techno");
  await list.click();
  await frame(page, "alone").getByRole("option", { name: "Chill" }).click();
  await expect(sent(page, "alone")).toContainText("Chill");
});

test("a button that changes its words keeps its width", async ({ page }) => {
  await page.goto("/#/top.blackout");
  const button = frame(page, "alone").getByRole("button");
  const before = await button.evaluate((el) => el.getBoundingClientRect().width);
  await button.click();
  await expect(button).toContainText("Blackout on");
  expect(await button.evaluate((el) => el.getBoundingClientRect().width)).toBe(before);
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
  ["top.live", "alone", "switch", "LIVE off", "SetLive on: true"],
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

test("Blackout: Space holds it, a short press latches it, the next one brings the lights back", async ({ page }) => {
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
  await button.click();
  await expect(button).toHaveText("Blackout");
  await expect(sent(page, "alone")).toHaveText("Blackout on: false");
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
  await expect(light).toHaveAttribute("data-standing", "lifted");
  await expect(sent(page, "alone")).toHaveText("Nothing sent yet");
  await page.mouse.up();
  await expect(sent(page, "alone")).toHaveText("SetAddress 20 to 26");
  await expect(light).toHaveAttribute("data-standing", "plain");
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
  await page.goto("/#/kit/Pad");
  await page.getByRole("button", { name: "5 of 5" }).click();
  await expect(page.locator('[data-kit-link="Pad"]')).toContainText("5");
  await page.goto("/#/");
  await expect(page.locator("[data-export]")).toHaveValue(/\| Pad \| `kit:Pad` \| Kit \| 5 of 5 \|/);
});

// ---- the review of the catalog (#188): each row a check, red before its fix

const away = [
  ["Space comes up with the focus outside the frame", async (page: Page) => {
    await page.getByLabel("What works, what does not?").focus();
    await page.keyboard.up("Space");
  }],
  ["the window loses the focus", async (page: Page) => {
    await page.evaluate(() => window.dispatchEvent(new Event("blur")));
  }],
] as const;
for (const [how, letGo] of away) {
  test(`UX-01 Blackout held by Space lets go once when ${how}`, async ({ page }) => {
    await page.goto("/#/top.blackout");
    const button = frame(page, "alone").getByRole("button");
    await button.focus();
    await page.keyboard.down("Space");
    await expect(sent(page, "alone")).toHaveText("Blackout on: true");
    await letGo(page);
    await expect(button).toHaveText("Blackout");
    await expect(sent(page, "alone")).toHaveText("Blackout on: false");
    await expect(page.locator("[data-log] li")).toHaveCount(2);
  });
}

test("UX-02 every control that is off without the engine looks off and says why", async ({ page }) => {
  const live: string[] = [];
  for (const control of CONTROLS) {
    const cell = control.cells["no-engine"];
    if (cell.is !== "off") continue;
    await page.goto(`/#/${control.id}`);
    const stage = frame(page, "no-engine").locator(".c-stage");
    await expect(stage).toBeVisible();
    const enabled = await stage.evaluate((el) => [...el.querySelectorAll<HTMLElement>("button, input, [tabindex]")].filter((t) => !(t as HTMLButtonElement).disabled && t.getAttribute("aria-disabled") !== "true").length);
    const text = await stage.innerText();
    if (enabled > 0 || !text.includes(cell.says)) live.push(`${control.id}: ${enabled} enabled, "${text.replace(/\s+/g, " ")}"`);
  }
  expect(live).toEqual([]);
});

// [control, keys, readout after, command sent]
const placings = [
  ["patch.grid.light", ["ArrowRight"], "18 to 24", "SetAddress 18 to 24"],
  ["patch.grid.light", ["ArrowDown"], "49 to 55", "SetAddress 49 to 55"],
  ["patch.grid.light", ["ArrowLeft", "ArrowLeft"], "15 to 21", "SetAddress 15 to 21"],
  ["looks.seq.bar", ["ArrowRight"], "At beat 1¼, 1 beat", "SetSequenceEntry At beat 1¼, 1 beat"],
  ["looks.seq.bar", ["Shift+ArrowRight"], "At beat 1, 1¼ beats", "SetSequenceEntry At beat 1, 1¼ beats"],
  ["looks.seq.bar", ["Shift+ArrowLeft", "Shift+ArrowLeft", "Shift+ArrowLeft", "Shift+ArrowLeft"], "At beat 1, ¼ beat", "SetSequenceEntry At beat 1, ¼ beat"],
] as const;
for (const [id, keys, readout, command] of placings) {
  test(`UX-05 ${id} moves by the keyboard: ${keys.join(", ")}`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    const bar = frame(page, "alone").getByRole("button");
    await bar.focus();
    for (const key of keys) await page.keyboard.press(key);
    await expect(bar).toContainText(readout);
    await expect(sent(page, "alone")).toHaveText(command);
  });
}

// [control, px dragged to the right, readout after]
const drags = [
  ["patch.grid.light", 80, "21 to 27"],
  ["patch.grid.light", -80, "13 to 19"],
  ["looks.seq.bar", 80, "At beat 2, 1 beat"],
] as const;
for (const [id, dx, readout] of drags) {
  test(`UX-05 ${id} dragged ${dx} px shows and sends where it landed`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    const bar = frame(page, "alone").getByRole("button");
    const box = await bar.boundingBox();
    if (!box) throw new Error("the bar is not on screen");
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 + dx, box.y + box.height / 2, { steps: 5 });
    await page.mouse.up();
    await expect(bar).toContainText(readout);
    await expect(sent(page, "alone")).toContainText(readout);
  });
}

// [control, the words before, the words while on, the command of the first press]
const toggles = [
  ["inout.out.test", "Test", "Testing", "TestOutput on: true"],
  ["inout.osc.on", "Off", "Listening", "SetOsc on: true"],
] as const;
for (const [id, before, on, command] of toggles) {
  test(`UX-07 ${id} starts off and its first press turns it on`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    const button = frame(page, "alone").getByRole("button");
    await expect(button).toHaveText(before);
    await expect(button).toHaveAttribute("aria-pressed", "false");
    await button.click();
    await expect(button).toHaveText(on);
    await expect(button).toHaveAttribute("aria-pressed", "true");
    await expect(sent(page, "alone")).toHaveText(command);
  });
}

test("UX-07 a test of an output ends by itself after three seconds, a press while it runs does nothing", async ({ page }) => {
  await page.goto("/#/inout.out.test");
  const button = frame(page, "alone").getByRole("button");
  await button.click();
  await expect(button).toHaveText("Testing");
  await button.click();
  await expect(button).toHaveText("Testing");
  await expect(page.locator("[data-log] li")).toHaveCount(1);
  await expect(button).toHaveText("Test", { timeout: 5000 });
  await expect(page.locator("[data-log] li")).toHaveCount(1);
});

test("UX-07 OSC goes back off on the second press", async ({ page }) => {
  await page.goto("/#/inout.osc.on");
  const button = frame(page, "alone").getByRole("button");
  await button.click();
  await button.click();
  await expect(button).toHaveText("Off");
  await expect(sent(page, "alone")).toHaveText("SetOsc on: false");
});

for (const id of ["top.workspaces", "console.speed", "inout.tempo.feed", "looks.pattern.shape"]) {
  test(`UX-08 arrows keep moving in ${id}: the picked option has the focus`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    await frame(page, "alone").locator('.k-pick-opt[tabindex="0"]').focus();
    const focused = () => page.evaluate(() => (document.activeElement?.classList.contains("k-pick-opt") ? document.activeElement.textContent : document.activeElement?.tagName));
    const start = await focused();
    await page.keyboard.press("ArrowRight");
    const once = await focused();
    await page.keyboard.press("ArrowRight");
    const twice = await focused();
    expect(new Set([start, once, twice]).size).toBe(3);
    await expect(frame(page, "alone").locator('.k-pick-opt[tabindex="0"]')).toBeFocused();
  });
}

// [situation, armed or writing]
const recording = [
  ["live-on", "armed"],
  ["lock", "armed"],
  ["no-song", "armed"],
  ["recording", "writing"],
] as const;
for (const [key, is] of recording) {
  test(`UX-09 the Recording lamp is red only while it writes: ${key}`, async ({ page }) => {
    await page.goto("/#/top.indicator.recording");
    const button = frame(page, key).getByRole("button");
    await expect(button).toHaveAttribute("aria-pressed", "true");
    if (is === "armed") {
      await expect(button.locator(".k-lamp")).not.toHaveAttribute("data-colour", "danger");
      await expect(button).toContainText("Armed");
    } else await expect(button.locator(".k-lamp")).toHaveAttribute("data-colour", "danger");
  });
}

test("UX-09 the Now strip follows the situation: its lanes and the two decks", async ({ page }) => {
  await page.goto("/#/top.now-strip");
  const says = async (key: string) => (await frame(page, key).locator(".c-stage").innerText()).replace(/\s+/g, " ");
  const texts = { usual: await says("alone"), song: await says("song"), autopilot: await says("autopilot"), empty: await says("empty"), noSong: await says("no-song") };
  expect(new Set(Object.values(texts)).size).toBe(Object.keys(texts).length);
  expect(texts.song).toMatch(/1\s?Playing/);
  expect(texts.autopilot).not.toMatch(/Autopilot\s?nothing/);
  expect(texts.noSong).toMatch(/1\s?Stopped/);
});

// [control, frame, the reason shown on the control]
const refusals = [
  ["patch.panel.add", "state:off: Universe 2 is already used", "Universe 2 is already used"],
  ["patch.add.next", "state:off: Pick a free address", "Pick a free address"],
] as const;
for (const [id, key, reason] of refusals) {
  test(`UX-11 ${id} is off and says why: ${reason}`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    const control = frame(page, key).locator(".c-stage button").first();
    await expect(control).toBeDisabled();
    await expect(control).toContainText(reason);
  });
}

// [control, typed, how it is left, sent, the value it keeps, the reason]
const typed = [
  ["inout.osc.port", "Banana", "Enter", "Nothing sent yet", "9000", "A port is a number from 1 to 65535"],
  ["inout.osc.port", "", "Enter", "Nothing sent yet", "9000", ""],
  ["inout.osc.port", "9001", "Tab", "SetOsc 9001", "9001", ""],
  ["inout.osc.port", "9001", "Enter", "SetOsc 9001", "9001", ""],
  ["inout.out.universes", "Banana", "Enter", "Nothing sent yet", "2 to 9", "One universe, as 1, or a range, as 2 to 9"],
  ["inout.out.universes", "9 to 2", "Tab", "Nothing sent yet", "2 to 9", "One universe, as 1, or a range, as 2 to 9"],
  ["inout.out.universes", "3", "Tab", "SetOutput 3", "3", ""],
  ["inout.mapping.message", "Banana", "Enter", "Nothing sent yet", "Note 36", "A note, as Note 36, or a controller, as CC 48"],
  ["inout.mapping.message", "CC 48", "Tab", "UpdateMapping CC 48", "CC 48", ""],
  ["inout.mapping.message", "", "Tab", "Nothing sent yet", "Note 36", ""],
  ["looks.name", "", "Enter", "Nothing sent yet", "Warm wash", ""],
] as const;
for (const [id, text, leave, command, keeps, reason] of typed) {
  test(`UX-31 ${id}: "${text}" and ${leave}`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    const field = frame(page, "alone").getByRole("textbox");
    await field.fill(text);
    await field.press(leave);
    await expect(sent(page, "alone")).toHaveText(command);
    if (reason) {
      await expect(frame(page, "alone").locator(".c-stage")).toContainText(reason);
      await expect(field).toHaveAttribute("aria-invalid", "true");
    } else await expect(frame(page, "alone").locator(".c-stage .k-reason")).toHaveCount(0);
    if (leave === "Tab" || !reason) await expect(field).toHaveValue(keeps);
  });
}
