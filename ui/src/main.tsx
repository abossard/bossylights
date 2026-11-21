import { For, onCleanup } from "solid-js";
import { render } from "solid-js/web";
import { connectEngine, engineAddress } from "./engine";
import { outputLines } from "./picture";
import "./style.css";

/** The status page until the approved screens: the connection, the engine and its outputs, nothing that changes them. */
function App() {
  const address = engineAddress();
  const engine = connectEngine(address);
  onCleanup(engine.close);
  const lines = () => {
    const picture = engine.picture();
    // The last picture says where each output sends; only a connected engine says how it is now.
    return picture ? outputLines(picture, engine.connection() === "connected" ? "now" : "before") : [];
  };
  return (
    <main>
      <p class="status" data-status={engine.connection()} data-testid="status">
        {engine.connection()}
      </p>
      <p class="label" data-testid="engine">
        {"url" in address ? `engine on ${address.url.host}` : `not an engine address: ${address.unusable}`}
      </p>
      <ul class="outputs" data-testid="outputs">
        <For each={lines()}>{(line) => <li class="label">{line}</li>}</For>
      </ul>
    </main>
  );
}

render(() => <App />, document.getElementById("root")!);
