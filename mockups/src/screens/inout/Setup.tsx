// Outputs, the audio input and OSC. Review 4 (issue #102, decision 29): no mockup had a control for these.
// They show the controls and say what would happen: the fake outputs are not changed.
import { createSignal, For, Show } from "solid-js";
import { Button, Segmented } from "../../kit";

const KINDS = ["DMX USB Pro", "DDP", "Art-Net", "sACN"];

/** One output, set up: what it is, which universes it carries, how often it sends. And Remove. */
export function OutputSetup(props: { name: string; live: boolean }) {
  const [open, setOpen] = createSignal(false);
  const [rate, setRate] = createSignal("40");
  const [said, setSaid] = createSignal("");
  return (
    <div class="tools">
      <Button pressed={open()} aria-label="Set up the output" aria-expanded={open()} onClick={() => setOpen(!open())}>
        {open() ? "Done" : "Set up"}
      </Button>
      <Show when={open()}>
        <label class="tools-field">
          Kind
          <select aria-label="Kind of the output" onChange={(e) => setSaid(`${props.name} sends as ${e.currentTarget.value}`)}>
            <For each={KINDS}>{(kind) => <option>{kind}</option>}</For>
          </select>
        </label>
        <label class="tools-field">
          Universes
          <input type="text" aria-label="Universes of the output" placeholder="1, or 2 to 9" onChange={(e) => setSaid(`${props.name} carries universe ${e.currentTarget.value}`)} />
        </label>
        <Segmented
          label="Frames a second"
          options={[
            { value: "30", label: "30" },
            { value: "40", label: "40" },
            { value: "44", label: "44" },
          ]}
          value={rate()}
          onChange={setRate}
        />
        <Button aria-label="Remove the output" disabled={props.live} reason="LIVE is on" onClick={() => setSaid(`${props.name} is removed. Its universes send nowhere`)}>
          Remove output
        </Button>
      </Show>
      <Show when={said()}>
        <span class="mute" role="status">
          {said()}
        </span>
      </Show>
    </div>
  );
}

/** One more output. It starts as the first kind, with no universe: "Set up" says the rest. */
export function AddOutput() {
  const [said, setSaid] = createSignal("");
  return (
    <div class="tools">
      <Button aria-label="Add an output" onClick={() => setSaid("A new output, not set up yet. It sends nothing until it has a universe")}>
        Add output
      </Button>
      <Show when={said()}>
        <span class="mute" role="status">
          {said()}
        </span>
      </Show>
    </div>
  );
}

/** Which input the beat is heard on. */
export function AudioInput() {
  return (
    <label class="tools-field">
      Listens to
      <select aria-label="Audio input">
        <For each={["Built-in input", "USB audio interface", "Loopback"]}>{(name) => <option>{name}</option>}</For>
      </select>
    </label>
  );
}

/** OSC: other software sends commands to a port of this machine. */
export function Osc() {
  const [on, setOn] = createSignal(false);
  return (
    <section class="io-sec">
      <h3 class="io-h">OSC</h3>
      <div class="tools">
        <Button pressed={on()} aria-label="OSC on or off" onClick={() => setOn(!on())}>
          {on() ? "Listening" : "Off"}
        </Button>
        <label class="tools-field">
          Port
          <input type="text" inputMode="numeric" aria-label="OSC port" value="9000" disabled={!on()} />
        </label>
      </div>
    </section>
  );
}
