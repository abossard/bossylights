import { For, onCleanup, Show } from "solid-js";
import { render } from "solid-js/web";
import { connectEngine, engineUrl } from "./engine";
import type { Priority, Refusal } from "./engine-api";
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
      <p class="label">engine frames, 44 a second</p>
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
