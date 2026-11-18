import { For, onCleanup, Show } from "solid-js";
import { render } from "solid-js/web";
import { connectEngine, engineUrl } from "./engine";
import type { Output, Priority, Refusal } from "./engine-api";
import "./style.css";

/** A developer aid: the first channels of universe 1. */
const ADDRESSES = [1, 2, 3, 4, 5, 6, 7, 8];

/** The output thread's priority, by name. */
function priorityText(priority: Priority | undefined): string {
  if (priority === undefined) return "not known yet";
  if (priority === "RealTime") return "real-time";
  const reason = priority.BestEffort.reason;
  if (reason === "NotSupported") return "best effort: not supported on this system";
  return `best effort: refused by the system (code ${reason.Refused.code})`;
}

/** Why a command was refused, by name. */
function refusalText(refusal: Refusal): string {
  if (typeof refusal === "string") return refusal;
  return `NoSuchUniverse: this engine has ${refusal.NoSuchUniverse.universes}`;
}

/** An output in one line: what it sends, how often, its state and counters. */
function outputText(number: number, output: Output): string {
  const { first, count } = output.universes;
  const status = typeof output.status === "string" ? output.status : `Reconnecting (${output.status.Reconnecting.failures})`;
  const c = output.counters;
  return `${number}: universes ${first} to ${first + count - 1}, ${output.rate} a second, ${status}, accepted ${c.accepted}, dropped ${c.dropped}, failed ${c.failed}, off beat ${c.off_beat}`;
}

function App() {
  const engine = connectEngine(engineUrl());
  onCleanup(engine.close);
  const level = (address: number) => engine.picture()?.universes[0]?.[address - 1] ?? 0;
  return (
    <main>
      <p class="status" data-status={engine.connection()} data-testid="status">
        {engine.connection()}
      </p>
      <p class="frame" data-testid="frame">
        {engine.picture()?.state.frame ?? "0"}
      </p>
      <p class="label">engine wakes</p>
      <p class="label" data-testid="priority">
        output thread: {priorityText(engine.picture()?.state.priority)}
      </p>
      <div class="channels">
        <For each={ADDRESSES}>
          {(address) => (
            <label class="channel">
              <span class="label">1.{address}</span>
              <input
                type="range"
                min="0"
                max="255"
                value={level(address)}
                data-testid={`fader-${address}`}
                onInput={(event) =>
                  engine.send({
                    SetChannel: { channel: { universe: 1, address }, value: Number(event.currentTarget.value) },
                  })
                }
              />
              <span class="level" data-testid={`level-${address}`}>
                {level(address)}
              </span>
            </label>
          )}
        </For>
      </div>
      <ul class="outputs" data-testid="outputs">
        <For each={engine.picture()?.state.outputs ?? []}>
          {(output, index) => (
            <Show when={output}>{(output) => <li class="label">{outputText(index() + 1, output())}</li>}</Show>
          )}
        </For>
      </ul>
      <Show when={engine.unsent()}>
        {(why) => (
          <p class="label" data-testid="unsent">
            {why()}
          </p>
        )}
      </Show>
      <Show when={engine.refusal()}>
        {(refusal) => (
          <p class="label" data-testid="refusal">
            refused: {refusalText(refusal())}
          </p>
        )}
      </Show>
    </main>
  );
}

render(() => <App />, document.getElementById("root")!);
