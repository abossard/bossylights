// Making, naming and removing looks, the items of a sequence, and a colour outside the swatches.
// Review 4 (issue #102, decision 29): no mockup had a control for these. They show the controls and say
// what would happen: the fake library is not changed.
import { createSignal, For, Show } from "solid-js";
import { store } from "../../fake/store";
import { Button } from "../../kit";

/** A new look, by its kind. In the library, beside "New group". */
export function NewLook() {
  const [said, setSaid] = createSignal("");
  const make = (kind: string) => setSaid(`A new ${kind} opens in the editor, its name ready to type`);
  return (
    <div class="tools" role="group" aria-label="New look">
      <Button aria-label="New step" onClick={() => make("step")}>
        New step
      </Button>
      <Button aria-label="New sequence" onClick={() => make("sequence")}>
        New sequence
      </Button>
      <Button aria-label="New pattern" onClick={() => make("pattern")}>
        New pattern
      </Button>
      <Show when={said()}>
        <span class="mute" role="status">
          {said()}
        </span>
      </Show>
    </div>
  );
}

/** The open look: its name, a copy, and Delete. In the head of the editor. */
export function LookTools(props: { id: string }) {
  const [said, setSaid] = createSignal("");
  const name = () => store.item(props.id)?.name ?? "";
  const used = () => store.usedIn(props.id).length;
  return (
    <div class="tools" role="group" aria-label="This look">
      <label class="tools-field">
        Name
        <input type="text" aria-label="Name of the look" value={name()} onChange={(e) => setSaid(`The look is called ${e.currentTarget.value || name()}, everywhere it is used`)} />
      </label>
      <Button aria-label="Copy the look" onClick={() => setSaid(`"${name()} copy" opens in the editor`)}>
        Copy
      </Button>
      <Button aria-label="Delete the look" disabled={used() > 0} reason={`Used in ${used()} sequences`} onClick={() => setSaid("The look is gone from the library, its pads and its sections. Undo puts it back")}>
        Delete look
      </Button>
      <Show when={said()}>
        <span class="mute" role="status">
          {said()}
        </span>
      </Show>
    </div>
  );
}

/** A sequence: one more item at its end, and the selected item out. */
export function SequenceItems(props: { seqName: string; selected?: string }) {
  const [said, setSaid] = createSignal("");
  return (
    <div class="tools" role="group" aria-label="Items of the sequence">
      <label class="tools-field">
        Add an item
        <select aria-label="Add an item to the sequence" onChange={(e) => (setSaid(`${e.currentTarget.value} joins ${props.seqName} at its end, one beat wide`), (e.currentTarget.selectedIndex = 0))}>
          <option>Pick a look</option>
          <For each={store.items().filter((i) => i.name !== props.seqName)}>{(i) => <option>{i.name}</option>}</For>
        </select>
      </label>
      <Button aria-label="Take the item out of the sequence" disabled={!props.selected} reason="Pick an item first" onClick={() => setSaid(`${props.selected} leaves ${props.seqName}. The look stays in the library`)}>
        Take out
      </Button>
      <Show when={said()}>
        <span class="mute" role="status">
          {said()}
        </span>
      </Show>
    </div>
  );
}

const hsl = (hue: number, sat: number) => `hsl(${Math.round(hue)} ${Math.round(sat)}% ${Math.round(100 - sat / 2)}%)`;

/** A colour outside the twelve swatches: a wheel. Around is the hue, outward is how strong it is. */
export function OtherColour(props: { full: boolean; onAdd: (colour: string) => void }) {
  const [open, setOpen] = createSignal(false);
  const [hue, setHue] = createSignal(30);
  const [sat, setSat] = createSignal(80);
  const at = (e: PointerEvent & { currentTarget: HTMLDivElement }) => {
    const box = e.currentTarget.getBoundingClientRect();
    const x = e.clientX - box.left - box.width / 2;
    const y = e.clientY - box.top - box.height / 2;
    setHue(((Math.atan2(y, x) * 180) / Math.PI + 450) % 360);
    setSat(Math.min(100, (Math.hypot(x, y) / (box.width / 2)) * 100));
  };
  const key = (e: KeyboardEvent) => {
    const by = e.shiftKey ? 10 : 1;
    if (e.key === "ArrowLeft") setHue((hue() - by + 360) % 360);
    else if (e.key === "ArrowRight") setHue((hue() + by) % 360);
    else if (e.key === "ArrowUp") setSat(Math.min(100, sat() + by));
    else if (e.key === "ArrowDown") setSat(Math.max(0, sat() - by));
    else return;
    e.preventDefault();
  };
  const spot = () => {
    const r = (sat() / 100) * 50;
    const a = ((hue() - 90) * Math.PI) / 180;
    return { left: `${50 + r * Math.cos(a)}%`, top: `${50 + r * Math.sin(a)}%`, background: hsl(hue(), sat()) };
  };
  return (
    <div class="tools">
      <Button pressed={open()} aria-label="Other colour" aria-expanded={open()} onClick={() => setOpen(!open())}>
        Other colour
      </Button>
      <Show when={open()}>
        <div
          class="colour-wheel"
          role="slider"
          tabIndex={0}
          aria-label="Colour wheel: hue around, strength outward"
          aria-valuetext={`Hue ${Math.round(hue())}, strength ${Math.round(sat())}`}
          onPointerDown={(e) => (e.currentTarget.setPointerCapture(e.pointerId), at(e))}
          onPointerMove={(e) => e.buttons === 1 && at(e)}
          onKeyDown={key}
        >
          <i style={spot()} />
        </div>
        <Button variant="primary" aria-label="Add this colour" disabled={props.full} reason="Four colours at most" onClick={() => props.onAdd(hsl(hue(), sat()))}>
          Add this colour
        </Button>
      </Show>
    </div>
  );
}
