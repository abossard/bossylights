import { onCleanup } from "solid-js";
import { render } from "solid-js/web";
import { connectEngine, engineUrl } from "./engine";
import "./style.css";

function App() {
  const engine = connectEngine(engineUrl());
  onCleanup(engine.close);
  return (
    <main>
      <p class="status" data-status={engine.status()} data-testid="status">
        {engine.status()}
      </p>
      <p class="frame" data-testid="frame">
        {engine.frame() ?? "0"}
      </p>
      <p class="label">engine frames, 44 a second</p>
    </main>
  );
}

render(() => <App />, document.getElementById("root")!);
