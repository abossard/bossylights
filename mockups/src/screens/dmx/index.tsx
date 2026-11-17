import type { Issue } from "../../shell/registry";
import { Stub } from "../../shell/Stub";

export const id = "dmx";
export const title = "DMX";
export const order = 7;
export const issues: Issue[] = [
  { number: 48, title: "DMX monitor and channel tester" },
  { number: 30, title: "DMX engine: universes, merge, masters" },
];

export function Screen() {
  return (
    <Stub
      areas={`"universes held" "channels held" "output output"`}
      columns="minmax(0, 3fr) minmax(0, 1fr)"
      rows="auto 1fr auto"
      regions={[
        { id: "universes", area: "universes", title: "Universes", line: "One tab per universe, with filters: patched only, changing." },
        { id: "channels", area: "channels", title: "Channels", line: "All 512 channels with value bars, grouped and labelled by fixture. Tap one to get a fader for it." },
        { id: "held", area: "held", title: "Held by hand", line: "Channels set by hand, in the Manual colour, with Release all." },
        { id: "output", area: "output", title: "Output", line: "Each universe's device, frames per second and errors." },
      ]}
    />
  );
}
