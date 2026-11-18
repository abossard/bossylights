// The page of one control: the element alone, a frame per situation and per state, all at once,
// what it sends, its gestures, its machine, and the owner's rating and comment.
import { createSignal, For, Show } from "solid-js";
import { COMMANDS } from "../controls/commands.ts";
import { MACHINES, type Machine } from "../controls/machines.ts";
import { GESTURES } from "../controls/situations.ts";
import { kindOf, primitiveOf, type Control } from "../controls/spec.ts";
import type { Sent } from "./act.ts";
import { framesOf, type Frame } from "./frames.ts";
import { FrameView } from "./FrameView.tsx";
import { propWords } from "./KitPage.tsx";
import type { Rating, Review } from "./review.ts";
import { ReviewBox } from "./ReviewBox.tsx";

const LOG = 8;

interface Logged {
  frame: string;
  sent: Sent;
}

export function Page(props: { control: Control; review: Review; onRate: (rating: Rating) => void; onComment: (text: string) => void }) {
  const c = props.control;
  const frames = framesOf(c);
  const of = (kind: Frame["kind"]) => frames.filter((f) => f.kind === kind);
  const machine = c.machine ? (MACHINES[c.machine] as Machine<string, string>) : undefined;
  // The last commands of this page, newest first.
  const [log, setLog] = createSignal<readonly Logged[]>([]);
  const logged = (frame: Frame, sent: readonly Sent[]) => setLog([...sent.map((s) => ({ frame: frame.title, sent: s })).reverse(), ...log()].slice(0, LOG));

  return (
    <article class="c-page" data-page={c.id} data-wide={c.kit === "Item.strip" ? "" : undefined}>
      <header class="c-page-head">
        <p class="c-crumb">
          <a href={`#/kit/${primitiveOf(c.kit)}`} data-kit-of>
            {primitiveOf(c.kit)}
          </a>
          {kindOf(c.kit) ? ` / ${kindOf(c.kit)}` : ""}
        </p>
        <h1>{c.name}</h1>
        <p class="c-does">{c.does}</p>
        <p class="c-tags">
          <code>{c.id}</code>
          <span class="c-tag" title="Where it lives">
            {c.screen}: {c.part}
          </span>
          <span class="c-tag">{c.kind}</span>
          <For each={propWords(c)}>{(word) => <span class="c-tag">{word}</span>}</For>
          <span class="c-tag" data-verdict={c.first.verdict}>
            {c.first.verdict}
          </span>
        </p>
      </header>

      <div class="c-top">
        <div class="c-alone">
          <For each={of("alone")}>{(frame) => <FrameView control={c} frame={frame} onSent={logged} />}</For>
        </div>

        <section class="c-facts" aria-label="What it does">
          <table>
            <tbody>
              <tr>
                <th>Sends</th>
                <td>
                  <For each={c.sends} fallback={`Nothing. It changes ${c.ui}`}>
                    {(name) => (
                      <p>
                        <code>{name}</code> {COMMANDS[name].does}
                        <Show when={COMMANDS[name].fields}> ({COMMANDS[name].fields})</Show>
                      </p>
                    )}
                  </For>
                </td>
              </tr>
              <tr>
                <th>On screen</th>
                <td>{c.feedback.screen}</td>
              </tr>
              <tr>
                <th>On a controller</th>
                <td>{c.feedback.controller}</td>
              </tr>
              <tr>
                <th>Undo</th>
                <td>
                  {c.undo.yes ? "Yes" : "No"}. {c.undo.why}
                </td>
              </tr>
              <tr>
                <th>Learn</th>
                <td>
                  {c.learn.yes ? "Yes" : "No"}. {c.learn.why}
                </td>
              </tr>
              <tr>
                <th>Function first</th>
                <td>
                  <b data-verdict={c.first.verdict}>{c.first.verdict}</b>. {c.first.why}
                </td>
              </tr>
            </tbody>
          </table>
        </section>

        <section class="c-review" aria-label="Your review">
          <ReviewBox review={props.review} onRate={props.onRate} onComment={props.onComment} />
          <h2>Sent on this page</h2>
          <ol class="c-log" data-log>
            <For each={log()} fallback={<li class="c-sent-none">Play a frame: the command shows as it fires</li>}>
              {(entry) => (
                <li>
                  <span class="c-command">
                    {entry.sent.name}
                    <Show when={entry.sent.value}>
                      {" "}
                      <span class="c-command-value">{entry.sent.value}</span>
                    </Show>
                  </span>
                  <span class="c-log-from">{entry.frame}</span>
                </li>
              )}
            </For>
          </ol>
        </section>
      </div>

      <h2>In each situation</h2>
      <div class="c-frames">
        <For each={of("situation")}>{(frame) => <FrameView control={c} frame={frame} onSent={logged} />}</For>
      </div>

      <Show when={of("state").length}>
        <h2>Its states</h2>
        <div class="c-frames">
          <For each={of("state")}>{(frame) => <FrameView control={c} frame={frame} onSent={logged} />}</For>
        </div>
      </Show>

      <h2>Gestures</h2>
      <table class="c-table">
        <tbody>
          <For each={GESTURES}>
            {(gesture) => (
              <tr>
                <th>{gesture}</th>
                <td>{c.gestures[gesture]}</td>
              </tr>
            )}
          </For>
        </tbody>
      </table>

      <Show when={machine}>
        {(m) => (
          <>
            <h2>State machine: {m().title}</h2>
            <div class="c-scroll">
              <table class="c-table c-machine">
                <thead>
                  <tr>
                    <th>State</th>
                    <th>On screen</th>
                    <For each={Object.keys(m().events)}>{(event) => <th title={m().events[event]}>{event}</th>}</For>
                  </tr>
                </thead>
                <tbody>
                  <For each={Object.entries(m().table)}>
                    {([state, row]) => (
                      <tr>
                        <th>{state}</th>
                        <td>{m().states[state]}</td>
                        <For each={Object.values(row)}>
                          {(step) => (
                            <td classList={{ "c-stays": step.to === state && !step.sends?.length }}>
                              {step.to === state ? "stays" : `to ${step.to}`}
                              <For each={step.sends ?? []}>{(name) => <span class="c-command">{name}</span>}</For>
                            </td>
                          )}
                        </For>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
            </div>
          </>
        )}
      </Show>
    </article>
  );
}
