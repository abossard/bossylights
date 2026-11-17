// Learn, across the whole app: press Learn, click any button, pad, fader or
// value on any screen, then move a hardware control. It is mapped and marked.
import { createRoot, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { store } from "../fake/store";
import { Button } from "../kit";
import { screens } from "./registry";
import { route } from "./router";
import "./learn.css";

interface Target {
  control: string;
  screen: string;
}

/** Fake hardware: what "moving a control" can be in the mockup. */
const HARDWARE = [
  { device: "Launchpad Mini", what: "a pad", message: () => `Note ${36 + Math.floor(Math.random() * 64)}`, feedback: true },
  { device: "Launchpad Mini", what: "a side button", message: () => `Note ${89 + Math.floor(Math.random() * 8)}`, feedback: true },
  { device: "APC mini", what: "a fader", message: () => `CC ${48 + Math.floor(Math.random() * 9)}`, feedback: false },
  { device: "APC mini", what: "a knob", message: () => `CC ${16 + Math.floor(Math.random() * 8)}`, feedback: false },
];

const CANDIDATE = "button, [role=slider], [role=switch], [role=radio], [role=tab]";

export const learn = createRoot(() => {
  const [step, setStep] = createSignal<"off" | "pick" | "press">("off");
  const [target, setTarget] = createSignal<Target | null>(null);
  const [message, setMessage] = createSignal("");
  return { step, setStep, target, setTarget, message, setMessage };
});

/** What a control is called: its accessible name, else its text. */
function nameOf(el: Element): string {
  const label = el.getAttribute("aria-label") ?? el.textContent ?? "";
  return label.replace(/\s+/g, " ").trim().slice(0, 48);
}
function screenOf(el: Element): string {
  if (el.closest(".topbar-wrap, .ns")) return "Top bar";
  return screens.find((s) => s.id === route())?.title ?? "Review";
}
const keyOf = (screen: string, control: string) => `${screen}|${control}`;

/** Marks every control on screen that has a mapping. */
function markMapped() {
  const keys = new Set(store.state.mappings.map((m) => keyOf(m.screen, m.control)));
  document.querySelectorAll(CANDIDATE).forEach((el) => {
    const on = keys.has(keyOf(screenOf(el), nameOf(el)));
    if (on) el.setAttribute("data-learned", "");
    else el.removeAttribute("data-learned");
  });
}

/** The learn button, the bar that guides the two steps, and the capture of clicks. */
export function LearnBar() {
  // in "pick", the next press on any control picks it instead of using it
  const capture = (e: Event) => {
    if (learn.step() !== "pick") return;
    const el = (e.target as Element).closest(CANDIDATE);
    if (!el || el.closest(".learn-bar, .learn-toggle")) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (e.type !== "click") return;
    learn.setTarget({ control: nameOf(el), screen: screenOf(el) });
    learn.setStep("press");
  };
  let observer: MutationObserver | undefined;
  let frame = 0;
  onMount(() => {
    for (const t of ["pointerdown", "pointerup", "click"]) document.addEventListener(t, capture, true);
    observer = new MutationObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(markMapped);
    });
    observer.observe(document.body, { childList: true, subtree: true });
    markMapped();
  });
  onCleanup(() => {
    for (const t of ["pointerdown", "pointerup", "click"]) document.removeEventListener(t, capture, true);
    observer?.disconnect();
  });

  const done = (hw: (typeof HARDWARE)[number]) => {
    const t = learn.target();
    if (!t) return;
    const message = hw.message();
    store.addMapping({ control: t.control, screen: t.screen, device: hw.device, message, feedback: hw.feedback });
    learn.setMessage(`Mapped: ${t.control} on ${hw.device}, ${message}.`);
    learn.setStep("off");
    learn.setTarget(null);
    requestAnimationFrame(markMapped);
    setTimeout(() => learn.setMessage(""), 4000);
  };
  const cancel = () => {
    learn.setStep("off");
    learn.setTarget(null);
  };

  return (
    <Show when={learn.step() !== "off" || learn.message()}>
      <div class="learn-bar" role="status">
        <Show when={learn.step() === "pick"}>
          <span class="learn-step">1 of 2</span>
          <b>Click the button, pad, fader or value you want to play from your controller.</b>
          <Button onClick={cancel}>Cancel</Button>
        </Show>
        <Show when={learn.step() === "press"}>
          <span class="learn-step">2 of 2</span>
          <b>Now move a control for “{learn.target()?.control}”.</b>
          <span class="learn-hw" role="group" aria-label="Your controller (simulated)">
            <For each={HARDWARE}>
              {(hw) => (
                <Button onClick={() => done(hw)}>
                  {hw.device}: {hw.what}
                </Button>
              )}
            </For>
          </span>
          <Button variant="quiet" onClick={cancel}>
            Cancel
          </Button>
        </Show>
        <Show when={learn.step() === "off" && learn.message()}>
          <b>{learn.message()}</b>
          <span class="mute">The list is on In/Out.</span>
        </Show>
      </div>
    </Show>
  );
}

export function LearnButton() {
  return (
    <Button
      class="learn-toggle"
      pressed={learn.step() !== "off"}
      onClick={() => {
        learn.setMessage("");
        learn.setStep(learn.step() === "off" ? "pick" : "off");
      }}
    >
      Learn
    </Button>
  );
}
