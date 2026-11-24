import { expect, test, type Locator, type Page } from "@playwright/test";
import { SITUATIONS, type SituationId } from "../controls/situations.ts";
import { PRIMITIVES } from "../controls/spec.ts";
import { CONTROLS } from "../controls/table.ts";
import { withReview } from "./reviews-file.mjs";

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
  ["menubar.stop-engine", "window-closed", "menuitem", /Stop engine/, "Stop"],
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

// [control, the top of its range, what End sends]. A level is 0 to 100, a raw DMX channel 0 to 255 (DMX512).
const faders = [
  ["console.fader", "100", /^SetMaster 100$/],
  ["dmx.channel.fader", "255", /^SetChannel .*255/],
] as const;
for (const [id, most, command] of faders) {
  test(`a fader follows the keyboard and sends its level: ${id} up to ${most}`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    const fader = frame(page, "alone").getByRole("slider");
    await expect(fader).toHaveAttribute("aria-valuemax", most);
    await fader.focus();
    await page.keyboard.press("End");
    await expect(fader).toHaveAttribute("aria-valuenow", most);
    await expect(sent(page, "alone")).toHaveText(command);
  });
}

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

// [px out, px back]: a thing that is not placed, dropped where it started, sends nothing
for (const [out, back] of [[60, -60], [200, -200], [0, 0]] as const) {
  test(`UX-05 songs.section.edge dragged ${out} px and back ${back} px sends nothing`, async ({ page }) => {
    await page.goto("/#/songs.section.edge");
    const edge = frame(page, "alone").getByRole("separator");
    const box = await edge.boundingBox();
    if (!box) throw new Error("the edge is not on screen");
    const [x, y] = [box.x + box.width / 2, box.y + box.height / 2];
    await page.mouse.move(x, y);
    await page.mouse.down();
    await page.mouse.move(x + out, y, { steps: 5 });
    await page.mouse.move(x + out + back, y, { steps: 5 });
    await page.mouse.up();
    await expect(sent(page, "alone")).toHaveText("Nothing sent yet");
  });
}

test("UX-05 songs.section.edge dragged 80 px sends where it landed", async ({ page }) => {
  await page.goto("/#/songs.section.edge");
  const edge = frame(page, "alone").getByRole("separator");
  const box = await edge.boundingBox();
  if (!box) throw new Error("the edge is not on screen");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 80, box.y + box.height / 2, { steps: 5 });
  await page.mouse.up();
  await expect(sent(page, "alone")).toHaveText("MoveSectionEdge where it landed");
});

// [control, the words before, the words while on, the command of the first press]
const toggles = [
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

test("UX-07 OSC goes back off on the second press", async ({ page }) => {
  await page.goto("/#/inout.osc.on");
  const button = frame(page, "alone").getByRole("button");
  await button.click();
  await button.click();
  await expect(button).toHaveText("Off");
  await expect(sent(page, "alone")).toHaveText("SetOsc on: false");
});

for (const id of ["top.workspaces", "console.speed", "inout.tempo.feed", "looks.pattern.shape", "looks.step.gobo"]) {
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
  test(`UX-08 a pick in ${id} keeps the option elements: nothing is drawn anew`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    const options = frame(page, "alone").locator(".k-pick-opt");
    await options.evaluateAll((nodes) => nodes.forEach((n) => n.setAttribute("data-kept", "")));
    await frame(page, "alone").locator('.k-pick-opt[tabindex="0"]').focus();
    await page.keyboard.press("ArrowRight");
    await expect(frame(page, "alone").locator(".k-pick-opt:not([data-kept])")).toHaveCount(0);
    await expect(options).not.toHaveCount(0);
  });
}

// [frame, armed or writing]: a song plays or not, whatever the lamp went through
const recording = [
  ["live-on", "armed"],
  ["lock", "armed"],
  ["no-song", "ready"],
  ["alone", "ready"],
  ["state:armed", "armed"],
  ["recording", "writing"],
  ["state:on", "writing"],
] as const;
for (const [key, is] of recording) {
  for (const presses of [0, 2]) {
    test(`UX-09 the Recording lamp is red only while it writes: ${key}, ${presses} presses`, async ({ page }) => {
      await page.goto("/#/top.indicator.recording");
      const button = frame(page, key).getByRole("button");
      for (let i = 0; i < presses; i++) await button.click();
      await expect(button).toHaveAttribute("aria-pressed", "true");
      if (is === "ready") {
        await expect(button.locator(".k-lamp")).not.toHaveAttribute("data-colour", "danger");
        await expect(button).toHaveAttribute("aria-label", /Ready/);
        await expect(button).not.toContainText("Armed");
      } else if (is === "armed") {
        await expect(button.locator(".k-lamp")).not.toHaveAttribute("data-colour", "danger");
        await expect(button).toContainText("Armed");
      } else await expect(button.locator(".k-lamp")).toHaveAttribute("data-colour", "danger");
    });
  }
}

/** Ready and On differ in shape and word, not only in fill (#194). Off: a dashed ring, no word, the name struck. `token`: the colour of the ring, and of the fill when solid. */
const LOCK = {
  ready: { solid: false, ring: "solid", token: "--mute", word: "Ready", says: "Ready, starts with LIVE" },
  on: { solid: true, ring: "solid", token: "--paper", word: "On", says: "On" },
  off: { solid: false, ring: "dashed", token: "--mute", word: "", says: "Off" },
} as const;
type LockState = keyof typeof LOCK;
/** What is painted, not only what is in the DOM. `reason`: why it is off. It stands in place of the word, on screen and in the name. */
const lockShows = async (button: Locator, state: LockState, reason = "") => {
  const { solid, ring, token, word, says } = LOCK[state];
  const lamp = button.locator(".k-lamp");
  await expect(lamp).toBeVisible();
  const painted = await lamp.evaluate((e, colour) => {
    const probe = document.body.appendChild(document.createElement("i"));
    probe.style.color = `var(${colour})`;
    const expected = getComputedStyle(probe).color;
    probe.remove();
    const style = getComputedStyle(e);
    return { expected, fill: style.backgroundColor, ring: style.borderTopColor, style: style.borderTopStyle, width: style.borderTopWidth };
  }, token);
  expect(painted.width, "the lamp has a ring").not.toBe("0px");
  expect(painted.style, `the ring is ${ring}`).toBe(ring);
  expect(painted.ring, `the ring is ${token}`).toBe(painted.expected);
  expect(painted.fill, `the lamp is ${solid ? "solid" : "empty"}`).toBe(solid ? painted.expected : "rgba(0, 0, 0, 0)");
  const line = reason || word;
  if (line === "") await expect(button.locator(".k-reason")).toHaveCount(0);
  else {
    await expect(button.locator(".k-reason")).toBeVisible();
    await expect(button.locator(".k-reason")).toHaveText(line);
  }
  const struck = await button.locator(".k-label").evaluate((e) => getComputedStyle(e).textDecorationLine);
  expect(struck.includes("line-through"), `the name is ${state === "off" ? "" : "not "}struck through`).toBe(state === "off");
  await expect(button).toHaveAccessibleName(`Lock: ${reason || says}`);
};
// [frame, the Lock's state there]
const lockFrames = [
  ["alone", "ready"],
  ["live-off", "ready"],
  ["state:ready", "ready"],
  ["live-on", "on"],
  ["lock", "on"],
  ["state:on", "on"],
  ["state:off", "off"],
] as const;
for (const [key, state] of lockFrames) {
  test(`#194 the Lock lamp says ${state} by shape and word: ${key}`, async ({ page }) => {
    await page.goto("/#/top.indicator.lock");
    await lockShows(frame(page, key).getByRole("button"), state);
  });
}
/** The Lock in each situation, written here, not read from the button: on while LIVE is on, ready otherwise; without the engine off with its reason. */
const LOCK_IN: Record<SituationId, readonly [LockState, string] | "gone"> = {
  "live-off": ["ready", ""],
  "live-on": ["on", ""],
  lock: ["on", ""],
  recording: ["on", ""],
  autopilot: ["ready", ""],
  holding: ["ready", ""],
  "no-engine": ["ready", "Engine not connected"],
  empty: ["ready", ""],
  "edit-surface": ["ready", ""],
  learn: ["ready", ""],
  song: ["ready", ""],
  "no-song": ["ready", ""],
  "output-lost": ["ready", ""],
  "window-closed": "gone",
};
test("#194 the Lock lamp shows its state by shape, word and name in every situation frame", async ({ page }) => {
  await page.goto("/#/top.indicator.lock");
  for (const { id } of SITUATIONS) {
    const button = frame(page, id).locator(".c-stage button");
    const is = LOCK_IN[id];
    if (is === "gone") await expect(button, id).toHaveCount(0);
    else {
      await expect(button, id).toHaveAttribute("data-state", is[0]);
      await lockShows(button, ...is);
    }
  }
});
for (const width of [320, 1280]) {
  test(`#194 at ${width} px the Lock keeps its width in every frame, a reason too: nothing beside it moves`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/#/top.indicator.lock");
    await expect(frame(page, "no-engine").getByRole("button")).toBeDisabled();
    const widths = await page.locator(".c-stage button[data-mark=lamp]").evaluateAll((buttons) =>
      buttons.map((b) => [b.closest("[data-frame]")?.getAttribute("data-frame"), Math.round(b.getBoundingClientRect().width)] as const),
    );
    const ready = widths.find(([key]) => key === "live-off")?.[1];
    expect(widths.length).toBeGreaterThan(lockFrames.length);
    expect(widths.filter(([, w]) => Math.abs(w - (ready ?? 0)) > 1)).toEqual([]);
  });
}

test("UX-09 the Now strip follows the situation: its lanes and the two decks", async ({ page }) => {
  await page.goto("/#/top.now-strip");
  const says = async (key: string) => (await frame(page, key).locator(".c-stage").innerText()).replace(/\s+/g, " ");
  const texts = { usual: await says("alone"), song: await says("song"), autopilot: await says("autopilot"), empty: await says("empty") };
  expect(new Set(Object.values(texts)).size).toBe(Object.keys(texts).length);
  expect(texts.autopilot).not.toMatch(/Autopilot\s?nothing/);
});

// [frame, a song plays]: only a frame with a song has a deck that plays and a Show lane that runs
const nowFrames = [
  ...["alone", "live-off", "live-on", "lock", "autopilot", "holding", "no-engine", "empty", "edit-surface", "learn", "no-song", "output-lost"].map((key) => [key, false] as const),
  ["song", true],
  ["recording", true],
] as const;
for (const [key, plays] of nowFrames) {
  test(`UX-09 the Now strip in ${key}: ${plays ? "a deck plays" : "no deck plays"}`, async ({ page }) => {
    await page.goto("/#/top.now-strip");
    const text = (await frame(page, key).locator(".c-stage").innerText()).replace(/\s+/g, " ");
    if (plays) expect(text).toMatch(/1 ?Playing/);
    else {
      expect(text).not.toMatch(/Playing/);
      expect(text).toMatch(/Show ?nothing/);
    }
  });
}

// [control, frame, the reason shown on the control]
const refusals = [
  ["patch.panel.add", "state:off: Universe 2 is already used", "Universe 2 is already used"],
  ["patch.add.next", "state:off: Pick a free address", "Pick a free address"],
  ["patch.panel.add", "state:off: It needs universes past 64", "It needs universes past 64"],
] as const;
for (const [id, key, reason] of refusals) {
  test(`UX-11 ${id} is off and says why: ${reason}`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    const control = frame(page, key).locator(".c-stage button").first();
    await expect(control).toBeDisabled();
    await expect(control).toContainText(reason);
  });
}

/** What a field of the panel's draft shows as sent: it changes the draft, Add the panel sends the panel. */
const PANEL = "Sends nothing: it changes the draft of the panel";
/** What a field that saves only on Enter says when it is left with a text not saved. */
const NOT_SAVED = "Not saved. Enter saves it";

// [control, typed, how it is left, sent, the value it keeps, the reason]
const typed = [
  ["inout.osc.port", "Banana", "Enter", "Nothing sent yet", "9000", "A port is a number from 1 to 65535"],
  ["inout.osc.port", "", "Enter", "Nothing sent yet", "9000", ""],
  ["inout.osc.port", "9001", "Tab", "SetOsc 9001", "9001", ""],
  ["inout.osc.port", "9001", "Enter", "SetOsc 9001", "9001", ""],
  ["inout.out.universes", "Banana", "Enter", "Nothing sent yet", "2 to 9", "One universe from 1 to 64, as 1, or a range, as 2 to 9"],
  ["inout.out.universes", "9 to 2", "Tab", "Nothing sent yet", "2 to 9", "One universe from 1 to 64, as 1, or a range, as 2 to 9"],
  ["inout.out.universes", "3", "Tab", "SetOutput 3", "3", ""],
  ["inout.mapping.message", "Banana", "Enter", "Nothing sent yet", "Note 36", "A note, as Note 36, or a controller, as CC 48"],
  ["inout.mapping.message", "CC 48", "Tab", "UpdateMapping CC 48", "CC 48", ""],
  ["inout.mapping.message", "", "Tab", "Nothing sent yet", "Note 36", ""],
  ["looks.name", "", "Enter", "Nothing sent yet", "Warm wash", ""],
  ["inout.osc.port", "007", "Enter", "SetOsc 7", "7", ""],
  ["inout.out.universes", "99999999", "Enter", "Nothing sent yet", "2 to 9", "One universe from 1 to 64, as 1, or a range, as 2 to 9"],
  ["inout.out.universes", "2 to 40000", "Tab", "Nothing sent yet", "2 to 9", "One universe from 1 to 64, as 1, or a range, as 2 to 9"],
  ["patch.panel.size", "Banana", "Enter", PANEL, "16 x 16", "Columns x rows, as 16 x 16, at most 10880 pixels"],
  ["patch.panel.size", "0", "Enter", PANEL, "16 x 16", "Columns x rows, as 16 x 16, at most 10880 pixels"],
  ["patch.panel.size", "-3", "Enter", PANEL, "16 x 16", "Columns x rows, as 16 x 16, at most 10880 pixels"],
  ["patch.panel.size", "Banana", "Tab", PANEL, "16 x 16", "Columns x rows, as 16 x 16, at most 10880 pixels"],
  ["patch.panel.size", "32 x 8", "Enter", PANEL, "32 x 8", ""],
  ["patch.panel.start", "40000", "Enter", PANEL, "2", "A universe is a number from 1 to 64"],
  ["patch.panel.start", "Banana", "Tab", PANEL, "2", "A universe is a number from 1 to 64"],
  ["inout.out.universes", "1 to 32768", "Enter", "Nothing sent yet", "2 to 9", "One universe from 1 to 64, as 1, or a range, as 2 to 9"],
  ["inout.out.universes", "2 to 64", "Enter", "SetOutput 2 to 64", "2 to 64", ""],
  ["patch.panel.start", "65", "Enter", PANEL, "2", "A universe is a number from 1 to 64"],
  ["patch.panel.start", "64", "Enter", PANEL, "64", ""],
  ["patch.panel.size", "1024 x 1024", "Enter", PANEL, "16 x 16", "Columns x rows, as 16 x 16, at most 10880 pixels"],
] as const;
// [control, typed and left by Tab, what stays in the field, it says it is not saved]: a field that saves only on Enter keeps a fitting text as a draft and says so
const drafts = [
  ["patch.panel.start", "7", "7", true],
  ["patch.panel.start", "007", "007", true],
  ["patch.panel.start", "", "2", false],
  ["patch.panel.size", "32 x 8", "32 x 8", true],
  ["patch.panel.size", "", "16 x 16", false],
  ["patch.panel.start", "2", "2", false],
] as const;
for (const [id, text, stays, draft] of drafts) {
  test(`UX-31 ${id}: "${text}" and Tab ${draft ? "stays a draft and says so" : "keeps the value"}`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    const field = frame(page, "alone").getByRole("textbox");
    await field.fill(text);
    await field.press("Tab");
    await expect(field).toHaveValue(stays);
    await expect(frame(page, "alone").locator(".c-stage .k-reason")).toHaveCount(0);
    const note = frame(page, "alone").locator(".c-stage .k-unsaved");
    if (!draft) return await expect(note).toHaveCount(0);
    await expect(note).toHaveText(NOT_SAVED);
    await field.press("Enter");
    await expect(note).toHaveCount(0);
  });
}

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

// ---- #189: the should-fix list and the nits

/** Drags the first control of a frame by `dy` px down. */
async function dragDown(page: Page, key: string, dy: number) {
  const control = frame(page, key).locator(".c-stage button").first();
  await control.scrollIntoViewIfNeeded();
  const box = await control.boundingBox();
  if (!box) throw new Error("the control is not on screen");
  await page.mouse.move(box.x + 20, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + 20, box.y + box.height / 2 + dy, { steps: 6 });
  await page.mouse.up();
}

test("UX-03 without the engine a look dragged onto a group is refused and says why", async ({ page }) => {
  await page.goto("/#/looks.library.look");
  await dragDown(page, "no-engine", 60);
  await expect(sent(page, "no-engine")).toHaveText("Refused: Engine not connected");
});

test("UX-03 a press opens the look and marks it, without the engine too", async ({ page }) => {
  await page.goto("/#/looks.library.look");
  for (const key of ["alone", "no-engine"]) {
    await frame(page, key).locator(".k-item").click();
    await expect(frame(page, key).locator('.k-item[data-standing="picked"]')).toHaveCount(1);
    await expect(sent(page, key)).toHaveText("Nothing sent yet");
  }
});

test("UX-06 Escape ends Learn from anywhere", async ({ page }) => {
  await page.goto("/#/top.learn");
  const learn = frame(page, "state:wait").getByRole("button");
  await expect(learn).toHaveAttribute("aria-pressed", "true");
  await page.locator("h1").click();
  await page.keyboard.press("Escape");
  await expect(learn).toHaveAttribute("aria-pressed", "false");
  await expect(sent(page, "state:wait")).toHaveText("CancelLearn");
});

test("UX-06 Escape ends Learn in a control that waits to be picked", async ({ page }) => {
  await page.goto("/#/top.tap");
  await expect(frame(page, "learn").locator(".c-stage.is-learn")).toHaveCount(1);
  await page.locator("h1").click();
  await page.keyboard.press("Escape");
  await expect(frame(page, "learn").locator(".c-stage.is-learn")).toHaveCount(0);
});

// [control, the options it shows, how many have a picture]
const pictured = [
  ["looks.pattern.shape", ["Plasma", "Stripes", "Wave", "Sparkle"], 4],
  ["patch.panel.wiring", ["Snake", "Zigzag"], 2],
  ["patch.add.model", ["LED PAR 64", "Moving head 250", "Strobe 1500"], 3],
  ["looks.step.colour.slot", ["Open", "Red", "Amber", "Green", "Blue", "Red and blue"], 6],
] as const;
for (const [id, names, pictures] of pictured) {
  test(`UX-10, UX-27 ${id} shows its real choices, each with a picture`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    const options = frame(page, "alone").locator(".k-pick-opt");
    await expect(options.locator(".k-label")).toHaveText([...names]);
    await expect(options.locator(".k-pick-picture")).toHaveCount(pictures);
  });
}

test("UX-12 a second tab rating another control keeps the first tab's rating", async ({ context }) => {
  const a = await context.newPage();
  const b = await context.newPage();
  await a.goto("/#/top.tap");
  await b.goto("/#/top.undo");
  await a.getByRole("button", { name: "4 of 5" }).click();
  await b.getByRole("button", { name: "3 of 5" }).click();
  await a.reload();
  await expect(a.locator(".c-rating-says")).toHaveText("4 of 5");
  await b.goto("/#/top.undo");
  await b.reload();
  await expect(b.locator(".c-rating-says")).toHaveText("3 of 5");
});

test("UX-26 with the feed the tempo source is words, not a button", async ({ page }) => {
  await page.goto("/#/top.tempo.source");
  await expect(frame(page, "state:from the feed").getByRole("button", { name: /from/ })).toHaveCount(0);
  await expect(frame(page, "state:from the feed").locator(".k-status")).toContainText("from VirtualDJ");
});

test("UX-26 the tempo source pressed gives the beat back and turns to words", async ({ page }) => {
  await page.goto("/#/top.tempo.source");
  await frame(page, "alone").getByRole("button", { name: /from Tap/ }).click();
  await expect(frame(page, "alone").getByRole("button", { name: /from/ })).toHaveCount(0);
  await expect(frame(page, "alone").locator(".k-status")).toContainText("from VirtualDJ");
});

test("UX-12 a review in another tab shows in this one without a reload", async ({ context }) => {
  const a = await context.newPage();
  const b = await context.newPage();
  await a.goto("/#/top.tap");
  await b.goto("/#/console.fader");
  await b.getByRole("button", { name: "2 of 5" }).click();
  await expect(a.locator('[data-link="console.fader"] .c-rated')).toHaveText("2");
});

test("UX-12 a review only in the file stays when another tab rates, and a comment keeps its rating", async ({ context }) => {
  let file = JSON.stringify({ "top.tap": { rating: 4, comment: "from the file" } });
  await context.route("**/reviews.json", (route) => {
    if (route.request().method() !== "PUT") return route.fulfill({ status: 200, contentType: "application/json", body: file });
    file = withReview(file, route.request().postData() ?? "") ?? file;
    return route.fulfill({ status: 204 });
  });
  const a = await context.newPage();
  const b = await context.newPage();
  await a.goto("/#/top.tap");
  await expect(a.locator(".c-rating-says")).toHaveText("4 of 5");
  await b.goto("/#/console.fader");
  await b.getByRole("button", { name: "2 of 5" }).click();
  await expect(a.locator('[data-link="console.fader"] .c-rated')).toHaveText("2");
  await expect(a.locator(".c-rating-says")).toHaveText("4 of 5");
  await expect(a.locator(".c-comment textarea")).toHaveValue("from the file");
  await a.locator(".c-comment textarea").press("End");
  await a.locator(".c-comment textarea").pressSequentially(" ok");
  await expect.poll((): unknown => JSON.parse(file)).toEqual({ "console.fader": { rating: 2, comment: "" }, "top.tap": { rating: 4, comment: "from the file ok" } });
  await a.goto("/#/");
  await expect(a.locator("textarea[data-export]")).toHaveValue(/from the file ok/);
});

test("UX-12 a review that is not written to the file says so", async ({ page }) => {
  await page.route("**/reviews.json", (route) => (route.request().method() === "PUT" ? route.fulfill({ status: 500 }) : route.continue()));
  await page.goto("/#/top.tap");
  await page.getByRole("button", { name: "4 of 5" }).click();
  await expect(page.locator("[data-unsaved]")).toContainText("Not written to the file");
});

/** The contrast of two colours as `rgb(…)`, after WCAG. */
const contrast = (a: string, b: string) => {
  const lum = (c: string) => {
    const [r, g, bl] = (c.match(/\d+(\.\d+)?/g) ?? []).slice(0, 3).map((v) => Number(v) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
    return 0.2126 * (r ?? 0) + 0.7152 * (g ?? 0) + 0.0722 * (bl ?? 0);
  };
  const [x, y] = [lum(a), lum(b)];
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
};

// [page, the edge that must be seen]
const edges = [
  ["inout.osc.port", ".k-input"],
  ["console.fader", ".k-fader-track"],
  ["console.xy", ".k-xy"],
  ["top.tap", ".c-comment textarea"],
] as const;
for (const [id, selector] of edges) {
  test(`UX-13 the edge of ${selector} reaches 3:1 against the stage`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    const edge = await page.locator(selector).first().evaluate((el) => getComputedStyle(el).borderTopColor);
    const stage = await page.locator(".c-stage").first().evaluate((el) => getComputedStyle(el).backgroundColor);
    expect(contrast(edge, stage)).toBeGreaterThanOrEqual(3);
  });
}

// [page, frame, the control]: marks drawn round a control, which the focus ring must still be seen over
const marked = [
  ["console.pad", "holding", ".k-pad"],
  ["looks.seq.bar", "state:dragged", ".k-block"],
  ["top.tap", "learn", ".c-stage button"],
  ["looks.step.colour.add", "alone", ".c-stage button"],
] as const;
for (const [id, key, selector] of marked) {
  test(`UX-14 the focus ring wins over the marks of ${id} in ${key}`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    const control = frame(page, key).locator(selector).first();
    const look = () => control.evaluate((el) => {
      const s = getComputedStyle(el);
      return { ring: `${s.outlineStyle} ${s.outlineWidth} ${s.outlineColor} ${s.outlineOffset}`, all: `${s.outline} ${s.outlineOffset} ${s.boxShadow}` };
    });
    const blurred = await look();
    await page.keyboard.press("Tab");
    await control.focus();
    const focused = await look();
    expect(focused.ring).toBe("solid 3px rgb(244, 239, 230) 2px");
    expect(focused.all).not.toBe(blurred.all);
  });
}

test("UX-14 a picked bar looks different with the focus", async ({ page }) => {
  await page.goto("/#/looks.seq.bar");
  const bar = frame(page, "alone").locator(".k-block");
  await bar.click();
  await expect(bar).toHaveAttribute("data-standing", "picked");
  const look = () => bar.evaluate((el) => `${getComputedStyle(el).outline} ${getComputedStyle(el).boxShadow}`);
  await page.locator("h1").click();
  const blurred = await look();
  await page.keyboard.press("Tab");
  await bar.focus();
  expect(await look()).not.toBe(blurred);
});

// [page, the control that is off without the engine]
const offline = [
  ["console.xy", ".k-xy"],
  ["console.fader", '[role="slider"]'],
] as const;
for (const [id, selector] of offline) {
  test(`UX-17 ${id} without the engine is said off, says why and keeps the focus`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    const control = frame(page, "no-engine").locator(selector);
    await expect(control).toHaveAttribute("aria-disabled", "true");
    await expect(control).toHaveAttribute("tabindex", "0");
    await expect(control).toHaveAccessibleDescription("Engine not connected");
  });
}

test("UX-17 an offline pad says why", async ({ page }) => {
  await page.goto("/#/console.pad");
  await expect(frame(page, "no-engine").locator(".k-pad")).toHaveAccessibleDescription("Engine not connected");
});

test("UX-18 the menu bar item opens its menu", async ({ page }) => {
  await page.goto("/#/menubar.item");
  const f = frame(page, "window-closed");
  await f.getByRole("button").first().click();
  await expect(f.getByRole("button").first()).toHaveAttribute("aria-expanded", "true");
  const menu = f.getByRole("menu");
  await expect(menu).toHaveAccessibleDescription(/^Running Frame \d/);
  await expect(menu.getByRole("menuitem")).toHaveText(["Open window", "Stop engine"]);
});

for (const [id, width] of [["top.now-strip", 400], ["top.workspaces", 400], ["top.workspaces", 1280], ["looks.seq.width", 1280], ["console.speed", 1280]] as const) {
  test(`UX-21, UX-22 ${id} at ${width} px: nothing runs past the page or its frame`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.goto(`/#/${id}`);
    await expect(page.locator(".c-frame").first()).toBeVisible();
    const main = await page.locator(".c-main").evaluate((el) => el.scrollWidth - el.clientWidth);
    expect(main).toBeLessThanOrEqual(1);
    const over = await page.locator(".c-stage").evaluateAll((stages) => stages.filter((s) => s.scrollWidth > s.clientWidth + 1).map((s) => s.closest(".c-frame")?.getAttribute("data-frame")));
    expect(over).toEqual([]);
  });
}

for (const width of [400, 1280]) {
  test(`UX-24 at ${width} px a page of tabs opens at its top, and a pick scrolls only the tab strip`, async ({ page }) => {
    await page.setViewportSize({ width, height: 700 });
    await page.goto("/#/top.workspaces");
    await expect(page.locator(".c-frame").first()).toBeVisible();
    await expect(page.locator("h1")).toBeInViewport();
    const f = frame(page, "live-on");
    await f.scrollIntoViewIfNeeded();
    const before = await page.evaluate(() => [document.querySelector(".c-main")?.scrollTop, document.scrollingElement?.scrollTop]);
    await f.getByRole("tab", { name: "In/Out" }).click();
    await expect(f.getByRole("tab", { name: "In/Out" })).toHaveAttribute("aria-selected", "true");
    expect(await page.evaluate(() => [document.querySelector(".c-main")?.scrollTop, document.scrollingElement?.scrollTop])).toEqual(before);
  });
}

test("UX-28 the tab of a lost output carries the trouble mark in its name", async ({ page }) => {
  await page.goto("/#/dmx.universe");
  await expect(frame(page, "output-lost").getByRole("tab", { name: "2 has trouble" })).toHaveCount(1);
});

test("UX-32 a lifted bar keeps its colour at full strength", async ({ page }) => {
  await page.goto("/#/looks.seq.bar");
  await expect(frame(page, "state:dragged").locator(".k-block")).toHaveCSS("opacity", "1");
});

test("UX-33 the explainer pad stops on a second press", async ({ page }) => {
  await page.goto("/#/how.pad");
  const pad = frame(page, "alone").locator(".k-pad");
  await pad.click();
  await expect(pad).toHaveAttribute("aria-pressed", "true");
  await pad.click();
  await expect(pad).toHaveAttribute("aria-pressed", "false");
  await expect(frame(page, "alone").locator(".c-sent-none")).toHaveText("Sends nothing: it changes the explainer's own model");
});

// [page, the target that must be 44 px both ways]
const targets = [
  ["console.speed", ".k-pick-opt"],
  ["songs.section.remove-look", ".k-item .k-btn"],
] as const;
for (const [id, selector] of targets) {
  test(`UX-15 ${id}: every target is 44 px both ways`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    const sizes = await frame(page, "alone").locator(selector).evaluateAll((els) => els.map((el) => el.getBoundingClientRect()).map((r) => Math.min(Math.round(r.width), Math.round(r.height))));
    expect(Math.min(...sizes)).toBeGreaterThanOrEqual(44);
  });
}

// [page, frame, key, what is sent]
const keys = [
  ["top.undo", "alone", "ControlOrMeta+z", "Undo"],
  ["top.redo", "state:has a step", "ControlOrMeta+Shift+z", "Redo"],
] as const;
for (const [id, key, keys_, command] of keys) {
  test(`UX-20 ${keys_} in a frame of ${id} sends ${command}`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    await frame(page, key).locator(".c-stage").click({ position: { x: 4, y: 4 } });
    await page.keyboard.press(keys_);
    await expect(sent(page, key)).toContainText(command);
  });
}

// [page, frame, the other control's key]
const notTheirs = [
  ["top.redo", "state:has a step", "ControlOrMeta+z"],
  ["top.undo", "alone", "ControlOrMeta+Shift+z"],
] as const;
for (const [id, key, keys_] of notTheirs) {
  test(`UX-20 ${keys_} in a frame of ${id} sends nothing`, async ({ page }) => {
    await page.goto(`/#/${id}`);
    await frame(page, key).locator(".c-stage").click({ position: { x: 4, y: 4 } });
    await page.keyboard.press(keys_);
    await expect(sent(page, key)).toHaveText("Nothing sent yet");
  });
}

test("UX-20 Redo with nothing to redo sends nothing", async ({ page }) => {
  await page.goto("/#/top.redo");
  await frame(page, "alone").locator(".c-stage").click({ position: { x: 4, y: 4 } });
  await page.keyboard.press("ControlOrMeta+Shift+z");
  await expect(sent(page, "alone")).toHaveText("Nothing sent yet");
});

test("UX-20 a number key in a frame of the workspaces picks that workspace", async ({ page }) => {
  await page.goto("/#/top.workspaces");
  await frame(page, "alone").locator(".c-stage").click({ position: { x: 4, y: 4 } });
  await page.keyboard.press("2");
  await expect(frame(page, "alone").getByRole("tab", { name: "Songs" })).toHaveAttribute("aria-selected", "true");
});

test("UX-23 at 400 px a specimen of the kit is not squeezed", async ({ page }) => {
  await page.setViewportSize({ width: 400, height: 900 });
  await page.goto("/#/kit/Block");
  const squeezed = await page.locator(".c-specimen .c-stage").evaluateAll((stages) => stages.filter((s) => s.scrollWidth > s.clientWidth + 1).length);
  expect(squeezed).toBe(0);
});

test("UX-30 the review export has a name", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("textbox", { name: "Review Markdown" })).toHaveCount(1);
});

// ---- #191: the catalog's leftovers, each row red before its fix

test("#191 G1 a review taken back in one tab does not come back from a comment in another", async ({ context }) => {
  let file = JSON.stringify({ "top.tap": { rating: 4, comment: "" } });
  await context.route("**/reviews.json", (route) => {
    if (route.request().method() === "PUT") file = withReview(file, route.request().postData() ?? "") ?? file;
    return route.fulfill({ status: 200, contentType: "application/json", body: file });
  });
  const a = await context.newPage();
  const b = await context.newPage();
  await a.goto("/#/top.tap");
  await b.goto("/#/top.tap");
  await expect(a.locator(".c-rating-says")).toHaveText("4 of 5");
  await expect(b.locator(".c-rating-says")).toHaveText("4 of 5");
  await b.getByRole("button", { name: "4 of 5" }).click();
  await expect.poll((): unknown => JSON.parse(file)).toEqual({});
  // the browser's store was empty and stays so: no store event tells A
  await expect(a.locator(".c-rating-says")).toHaveText("4 of 5");
  await a.locator(".c-comment textarea").fill("note from A");
  await expect.poll((): unknown => JSON.parse(file)).toEqual({ "top.tap": { rating: 0, comment: "note from A" } });
  await expect(a.locator(".c-rating-says")).toHaveText("Not rated");
  await a.goto("/#/");
  await expect(a.locator("textarea[data-export]")).not.toHaveValue(/4 of 5/);
});

test("#191 G1 an answer of the file that comes late does not undo what another tab wrote since", async ({ context }) => {
  let file = JSON.stringify({ "top.tap": { rating: 4, comment: "" } });
  let late: (() => Promise<void>) | undefined;
  const a = await context.newPage();
  const b = await context.newPage();
  await context.route("**/reviews.json", (route) => {
    if (route.request().method() !== "PUT") return route.fulfill({ status: 200, contentType: "application/json", body: file });
    file = withReview(file, route.request().postData() ?? "") ?? file;
    const answer = file;
    // B's answer is held until A has written: it is older than what A wrote
    if (route.request().frame().page() !== b) return route.fulfill({ status: 200, contentType: "application/json", body: answer });
    late = () => route.fulfill({ status: 200, contentType: "application/json", body: answer });
  });
  await a.goto("/#/top.tap");
  await b.goto("/#/top.tap");
  await expect(b.locator(".c-rating-says")).toHaveText("4 of 5");
  await b.getByRole("button", { name: "4 of 5" }).click();
  await expect.poll((): unknown => JSON.parse(file)).toEqual({});
  await a.locator(".c-comment textarea").fill("note from A");
  await expect(b.locator(".c-comment textarea")).toHaveValue("note from A");
  await late?.();
  await expect.poll((): unknown => JSON.parse(file)).toEqual({ "top.tap": { rating: 0, comment: "note from A" } });
  for (const page of [a, b]) {
    await expect(page.locator(".c-comment textarea")).toHaveValue("note from A");
    await expect(page.locator(".c-rating-says")).toHaveText("Not rated");
  }
});

const TABS = CONTROLS.filter((c) => c.kit === "Picker.tabs").map((c) => c.id);

for (const id of TABS) {
  test(`#191 G2 at 1280 px every tab strip of ${id} shows all its tabs, in every frame`, async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto(`/#/${id}`);
    await expect(page.locator(".c-frame").first()).toBeVisible();
    const cut = await page.locator('.c-frame .k-pick[data-kind="tabs"]').evaluateAll((strips) => strips.filter((s) => s.scrollWidth > s.clientWidth + 1).map((s) => s.closest(".c-frame")?.getAttribute("data-frame")));
    expect(cut).toEqual([]);
  });

  test(`#191 F-ring the focus ring of every tab of ${id} is drawn whole`, async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto(`/#/${id}`);
    const tabs = frame(page, "alone").getByRole("tab");
    await expect(tabs.first()).toBeVisible();
    for (const tab of await tabs.all()) {
      await tab.focus();
      // the ring's box, and every box around it that clips: the ring must lie inside each
      const clipped = await tab.evaluate((el) => {
        const style = getComputedStyle(el);
        if (!el.matches(":focus-visible") || style.outlineStyle === "none") return "no ring";
        const out = parseFloat(style.outlineWidth) + parseFloat(style.outlineOffset);
        const r = el.getBoundingClientRect();
        const ring = { left: r.left - out, top: r.top - out, right: r.right + out, bottom: r.bottom + out };
        for (let up = el.parentElement; up; up = up.parentElement) {
          const s = getComputedStyle(up);
          if (s.overflowX === "visible" && s.overflowY === "visible") continue;
          const b = up.getBoundingClientRect();
          const box = { left: b.left + up.clientLeft, top: b.top + up.clientTop, right: b.left + up.clientLeft + up.clientWidth, bottom: b.top + up.clientTop + up.clientHeight };
          if (ring.left < box.left - 0.5 || ring.top < box.top - 0.5 || ring.right > box.right + 0.5 || ring.bottom > box.bottom + 0.5) return up.className;
        }
        return "";
      });
      expect(clipped, await tab.textContent() ?? "").toBe("");
    }
  });
}

for (const width of [400, 1280]) {
  test(`#191 F-row at ${width} px no control runs past its stage, on any page`, async ({ page }) => {
    test.setTimeout(180_000);
    await page.setViewportSize({ width, height: 900 });
    const over: string[] = [];
    for (const control of CONTROLS) {
      await page.goto(`/#/${control.id}`);
      await expect(page.locator(`[data-page="${control.id}"] h1`)).toHaveText(control.name);
      const frames = await page.locator(".c-stage").evaluateAll((stages) => stages.filter((s) => s.scrollWidth > s.clientWidth + 1).map((s) => s.closest(".c-frame")?.getAttribute("data-frame")));
      over.push(...frames.map((f) => `${control.id} ${f}`));
    }
    expect(over).toEqual([]);
  });
}

test("#191 N Escape in the review notes keeps Learn", async ({ page }) => {
  await page.goto("/#/top.learn");
  const learn = frame(page, "state:wait").getByRole("button");
  await expect(learn).toHaveAttribute("aria-pressed", "true");
  await page.getByLabel("What works, what does not?").focus();
  await page.keyboard.press("Escape");
  await expect(learn).toHaveAttribute("aria-pressed", "true");
});

test("#191 N Escape in one frame ends Learn in that frame only", async ({ page }) => {
  await page.goto("/#/top.learn");
  const wait = frame(page, "state:wait").getByRole("button");
  const pick = frame(page, "state:pick").getByRole("button");
  await expect(wait).toHaveAttribute("aria-pressed", "true");
  await expect(pick).toHaveAttribute("aria-pressed", "true");
  await wait.focus();
  await page.keyboard.press("Escape");
  await expect(wait).toHaveAttribute("aria-pressed", "false");
  await expect(pick).toHaveAttribute("aria-pressed", "true");
});

test("#191 N a field left with its own value between spaces shows it without them", async ({ page }) => {
  await page.goto("/#/patch.panel.size");
  const field = frame(page, "alone").getByRole("textbox");
  await field.fill(" 16 x 16 ");
  await field.press("Enter");
  await expect(field).toHaveValue("16 x 16");
});
