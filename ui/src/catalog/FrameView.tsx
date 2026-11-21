// One frame of the storyboard: the control, live, with what is true here and what it sends as it fires.
import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { COMMANDS } from "../controls/commands.ts";
import type { Control } from "../controls/spec.ts";
import { act, type Act, type Sent } from "./act.ts";
import type { Frame } from "./frames.ts";
import { Live } from "./Live.tsx";
import type { Look } from "./look.ts";
import { USES } from "./uses.ts";

const MARK = { works: "works", off: "off", gone: "not there" } as const;

const command = (e: KeyboardEvent) => e.metaKey || e.ctrlKey;
/** Keys a row takes from anywhere outside a text field: here, from anywhere in its frame. Data. */
const FRAME_KEYS: Record<string, (e: KeyboardEvent, look: Look) => Act | undefined> = {
  "top.undo": (e) => (command(e) && !e.shiftKey && e.key.toLowerCase() === "z" ? { is: "press" } : undefined),
  "top.redo": (e) => (command(e) && e.shiftKey && e.key.toLowerCase() === "z" ? { is: "press" } : undefined),
  "top.workspaces": (e, look) => {
    const option = !command(e) && !e.altKey && /^[1-9]$/.test(e.key) ? look.options[Number(e.key) - 1] : undefined;
    return option === undefined ? undefined : { is: "pick", option };
  },
};

/** A key that is typed into a field is the field's. */
const typing = (target: EventTarget | null) => target instanceof HTMLElement && (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName));

export function FrameView(props: { control: Control; frame: Frame; onSent: (frame: Frame, sent: readonly Sent[]) => void }) {
  // What the frame shows now. It starts as the row says and changes as it is played: the screen's own state.
  const [look, setLook] = createSignal(props.frame.look);
  const [sent, setSent] = createSignal<readonly Sent[]>([]);
  const [fired, setFired] = createSignal(0);
  const holds = USES[props.control.id]?.on.keyDown !== undefined;
  let swallowClick = false;

  const play = (gesture: Act) => {
    const result = act(props.control, look(), gesture);
    setLook(result.look);
    if (result.sent.length === 0) return;
    setSent(result.sent);
    setFired(fired() + 1);
    props.onSent(props.frame, result.sent);
  };

  // Escape ends Learn from anywhere but a text field and another frame: the Learn button that waits, and a control that waits to be picked.
  const learns = USES[props.control.id]?.machine === "learn";
  let section: HTMLElement | undefined;
  const escape = (e: KeyboardEvent) => {
    if (e.key !== "Escape" || typing(e.target)) return;
    const inFrame = e.target instanceof Element ? e.target.closest(".c-frame") : null;
    if (inFrame !== null && inFrame !== section) return;
    if (look().learn || (learns && look().at !== "off")) play({ is: "cancel" });
  };
  onMount(() => window.addEventListener("keydown", escape));
  onCleanup(() => window.removeEventListener("keydown", escape));
  // The keys a row takes from anywhere, from anywhere in this frame.
  const frameKey = (e: KeyboardEvent) => {
    const gesture = typing(e.target) ? undefined : FRAME_KEYS[props.control.id]?.(e, look());
    if (!gesture) return;
    e.preventDefault();
    play(gesture);
  };

  // Learn waits: the press picks the control, the control never sees it.
  const pick = (e: Event) => {
    if (!look().learn) return;
    e.stopPropagation();
    e.preventDefault();
    if (e.type === "click") play({ is: "press" });
  };
  // A key that holds: dark while Space is down. It lets go once: Space comes up anywhere, or the window loses the focus.
  // The click Space would make on the way up is swallowed.
  let holding = false;
  const letGo = () => {
    if (!holding) return;
    holding = false;
    window.removeEventListener("keyup", spaceUp);
    window.removeEventListener("blur", letGo);
    play({ is: "key", down: false });
  };
  const spaceUp = (e: KeyboardEvent) => e.key === " " && letGo();
  onCleanup(() => {
    window.removeEventListener("keyup", spaceUp);
    window.removeEventListener("blur", letGo);
  });
  const key = (down: boolean) => (e: KeyboardEvent) => {
    // Some browsers still click when Space comes up. That click comes before the next key goes down, or never.
    if (down) swallowClick = false;
    if (!holds || e.key !== " " || look().learn) return;
    e.preventDefault();
    if (e.repeat) return;
    swallowClick = !down;
    if (!down) return letGo();
    if (holding) return;
    holding = true;
    window.addEventListener("keyup", spaceUp);
    window.addEventListener("blur", letGo);
    play({ is: "key", down: true });
  };
  const click = (e: MouseEvent) => {
    if (swallowClick && e.detail === 0) {
      swallowClick = false;
      e.stopPropagation();
      return;
    }
    pick(e);
  };

  // The stage hears a press before the control does: on the way in, not on the way out.
  const beforeTheControl = (stage: HTMLDivElement) => {
    stage.addEventListener("pointerdown", pick, { capture: true });
    stage.addEventListener("click", click, { capture: true });
  };

  return (
    // tabindex -1: a press anywhere in the frame takes the focus into it, so its keys reach it.
    <section ref={(el) => (section = el)} class="c-frame" data-frame={props.frame.key} data-is={props.frame.is} data-kind={props.frame.kind} tabindex="-1" onKeyDown={frameKey}>
      <header class="c-frame-head">
        <h3>{props.frame.title}</h3>
        <span class="c-is" data-is={props.frame.is}>
          {MARK[props.frame.is]}
        </span>
      </header>
      <Show when={props.frame.fact}>
        <p class="c-fact">{props.frame.fact}</p>
      </Show>
      <div class="c-stage" classList={{ "is-learn": look().learn }} ref={beforeTheControl} onKeyDown={key(true)} onKeyUp={key(false)}>
        <Show when={!look().gone} fallback={<span class="c-gone">Not there</span>}>
          <Live control={props.control} look={look()} act={play} />
        </Show>
      </div>
      <p class="c-says">{props.frame.says}</p>
      <p class="c-sent" data-sent aria-live="polite">
        <Show when={!look().refused} fallback={<span class="c-refused">Refused: {look().refused}</span>}>
        <Show when={sent().length} fallback={<span class="c-sent-none">{props.control.sends.length || look().learn ? "Nothing sent yet" : `Sends nothing: it changes ${props.control.ui}`}</span>}>
          <For each={sent()}>
            {(s) => (
              <span class="c-command" data-fired={fired()} title={`${COMMANDS[s.name].does}${COMMANDS[s.name].fields ? `. Fields: ${COMMANDS[s.name].fields}` : ""}`}>
                {s.name}
                <Show when={s.value}>
                  {" "}
                  <span class="c-command-value">{s.value}</span>
                </Show>
              </span>
            )}
          </For>
        </Show>
        </Show>
      </p>
    </section>
  );
}
