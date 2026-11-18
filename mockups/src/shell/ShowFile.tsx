import { createEffect, createSignal, Show } from "solid-js";
import { store, type ClockFeed } from "../fake/store";
import { Button } from "../kit";

/** The show file: its name, and new, open, save as. Review 4: no mockup had a control for these. */
export function ShowFile() {
  const [open, setOpen] = createSignal(false);
  const [name, setName] = createSignal(store.state.showName);
  const [said, setSaid] = createSignal("");
  const say = (text: string) => (setSaid(text), setOpen(false));
  return (
    <span class="tools-anchor">
      <Button variant="quiet" pressed={open()} aria-label="Show file" aria-haspopup="true" onClick={() => setOpen(!open())}>
        {name()}
      </Button>
      <Show when={said()}>
        <span class="mute" role="status">
          {said()}
        </span>
      </Show>
      <Show when={open()}>
        <div class="tools-pop tools tools-col" role="group" aria-label="Show file">
          <label class="tools-field">
            Name of the show
            <input type="text" aria-label="Name of the show" value={name()} onChange={(e) => setName(e.currentTarget.value || name())} />
          </label>
          <Button aria-label="New show file" onClick={() => (setName("New show"), say("A new, empty show file"))}>
            New
          </Button>
          <Button aria-label="Open a show file" onClick={() => say("The system asks which file to open")}>
            Open…
          </Button>
          <Button aria-label="Save the show file as" onClick={() => say("The system asks where to save a copy")}>
            Save as…
          </Button>
        </div>
      </Show>
    </span>
  );
}

/**
 * Who drives the beat, beside the tempo. While taps hold it, the words are a button that gives the beat
 * back to the feed that was picked (decision 23).
 */
export function TempoSource() {
  const FEEDS: Record<ClockFeed, string> = { tap: "Tap", audio: "Audio", midi: "MIDI clock", vdj: "VirtualDJ", timecode: "Timecode" };
  // The feed picked on In/Out, kept while taps hold the beat.
  const [picked, setPicked] = createSignal<ClockFeed>("vdj");
  const feed = () => store.state.clockFeed;
  createEffect(() => {
    if (feed() !== "tap") setPicked(feed());
  });
  return (
    <Show when={feed() === "tap"} fallback={<span class="mute">from {FEEDS[feed()]}</span>}>
      <button type="button" class="from-tap" aria-label="From Tap: give the beat back to the feed" onClick={() => store.setClockFeed(picked())}>
        from Tap
      </button>
    </Show>
  );
}
