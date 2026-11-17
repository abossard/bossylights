import type { Issue } from "../../shell/registry";
import { Stub } from "../../shell/Stub";

export const id = "inout";
export const title = "In/Out";
export const order = 7;
export const issues: Issue[] = [
  { number: 42, title: "USB DMX adapters" },
  { number: 43, title: "Art-Net and sACN" },
  { number: 44, title: "DDP pixel output" },
  { number: 33, title: "MIDI in with learn" },
  { number: 92, title: "Ready-made controller profiles" },
  { number: 34, title: "OSC in and out" },
  { number: 35, title: "Audio in: beat and levels" },
  { number: 45, title: "One tempo for everything" },
];

export function Screen() {
  return (
    <Stub
      areas={`"in clock out"`}
      columns="repeat(3, minmax(0, 1fr))"
      rows="1fr"
      regions={[
        { id: "in", area: "in", title: "In", line: "Controllers with profile and state, audio input with a level meter, the VirtualDJ link with deck states, OSC. MIDI learn starts here." },
        { id: "clock", area: "clock", title: "Clock", line: "Which feed drives the beat (Tap, Audio, MIDI clock, VirtualDJ, Timecode), each with its tempo and confidence, one selected." },
        { id: "out", area: "out", title: "Out", line: "Universes and their devices (USB DMX, Art-Net, sACN, DDP) with state, rate and a test button." },
      ]}
    />
  );
}
