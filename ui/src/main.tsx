import { onCleanup } from "solid-js";
import { render } from "solid-js/web";
import { connectEngine, engineUrl } from "./engine";
import type { Priority } from "./engine-api";
import "./style.css";

/** The output thread's priority, by name. */
function priorityText(priority: Priority | undefined): string {
  if (priority === undefined) return "not known yet";
  if (priority === "RealTime") return "real-time";
  const reason = priority.BestEffort.reason;
  if (reason === "NotSupported") return "best effort: not supported on this system";
  return `best effort: refused by the system (code ${reason.Refused.code})`;
}

function App() {
  const engine = connectEngine(engineUrl());
  onCleanup(engine.close);
  return (
    <main>
      <p class="status" data-status={engine.connection()} data-testid="status">
        {engine.connection()}
      </p>
      <p class="frame" data-testid="frame">
        {engine.frame() ?? "0"}
      </p>
      <p class="label">engine frames, 44 a second</p>
      <p class="label" data-testid="priority">
        output thread: {priorityText(engine.priority())}
      </p>
    </main>
  );
}

render(() => <App />, document.getElementById("root")!);
