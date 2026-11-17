import type { Issue } from "../../shell/registry";
import { Stub } from "../../shell/Stub";

export const id = "songs";
export const title = "Songs";
export const order = 3;
export const issues: Issue[] = [
  { number: 96, title: "Song Director" },
  { number: 47, title: "Song analysis: grid, sections, energy" },
  { number: 52, title: "VirtualDJ link" },
];

export function Screen() {
  return (
    <Stub
      areas={`"library song looks"`}
      columns="minmax(0, 1fr) minmax(0, 2fr) minmax(0, 1fr)"
      rows="1fr"
      regions={[
        { id: "library", area: "library", title: "Song library", line: "Every song with its analysis state (Analysed, Queued, Needs review), tempo and how sure the analysis is." },
        { id: "song", area: "song", title: "Song", line: "The selected song: waveform, the section ruler (intro, build, drop, break, outro), one block of looks per section, and the energy curve." },
        { id: "looks", area: "looks", title: "Looks to add", line: "Looks to drag into a section, grouped as Wash, Movement, Strobe and Pixels." },
      ]}
    />
  );
}
