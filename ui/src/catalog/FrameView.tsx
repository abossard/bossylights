// One frame of the storyboard: the control, live, with what is true here and what it sends as it fires.
import { createSignal, For, onCleanup, Show } from "solid-js";
import { COMMANDS } from "../controls/commands.ts";
import type { Control } from "../controls/spec.ts";
import { act, ENDS_BY_ITSELF, type Act, type Sent } from "./act.ts";
import type { Frame } from "./frames.ts";
import { Live } from "./Live.tsx";
import { inState } from "./look.ts";
import { USES } from "./uses.ts";

const MARK = { works: "works", off: "off", gone: "not there" } as const;

export function FrameView(props: { control: Control; frame: Frame; onSent: (frame: Frame, sent: readonly Sent[]) => void }) {
  // What the frame shows now. It starts as the row says and changes as it is played: the screen's own state.
  const [look, setLook] = createSignal(props.frame.look);
  const [sent, setSent] = createSignal<readonly Sent[]>([]);
  const [fired, setFired] = createSignal(0);
  const holds = USES[props.control.id]?.on.keyDown !== undefined;
  let swallowClick = false;

  // A toggle that ends by itself is back after a while: the engine ends it, nothing is sent.
  const ends = ENDS_BY_ITSELF[props.control.id];
  let ending: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(ending));

  const play = (gesture: Act) => {
    const result = act(props.control, look(), gesture);
    setLook(result.look);
    if (ends && result.look.label === ends.while && ending === undefined) {
      ending = setTimeout(() => {
        ending = undefined;
        setLook(inState(props.control, ends.back));
      }, ends.ms);
    }
    if (result.sent.length === 0) return;
    setSent(result.sent);
    setFired(fired() + 1);
    props.onSent(props.frame, result.sent);
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
    <section class="c-frame" data-frame={props.frame.key} data-is={props.frame.is} data-kind={props.frame.kind}>
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
      </p>
    </section>
  );
}
