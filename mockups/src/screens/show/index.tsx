import type { Issue } from "../../shell/registry";
import { Stub } from "../../shell/Stub";

export const id = "show";
export const title = "Show";
export const order = 3;
export const nowStrip = false;
export const issues: Issue[] = [
  { number: 85, title: "Show timeline with waveform" },
  { number: 94, title: "Record what you busk" },
  { number: 95, title: "Follow timecode" },
  { number: 99, title: "DJ set shows" },
  { number: 70, title: "Cue lists" },
];

export function Screen() {
  return (
    <Stub
      areas={`"transport transport" "shows timeline"`}
      columns="minmax(0, 1fr) minmax(0, 3fr)"
      rows="auto 1fr"
      regions={[
        { id: "transport", area: "transport", title: "Transport", line: "Play, stop and record, the Edit, Record and Run modes, and what the show follows: Internal clock, VirtualDJ deck A or Timecode." },
        { id: "shows", area: "shows", title: "Shows", line: "The list of shows. A recorded set holds one show per song." },
        { id: "timeline", area: "timeline", title: "Timeline", line: "Time and bar ruler, waveform, section ruler, tracks for Looks, Movers, Pixels, Laser and Recorded, clips, and the NOW line as the playhead. In Run mode a large Now and Next list with Go and Back takes over." },
      ]}
    />
  );
}
