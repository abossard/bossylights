import type { Issue } from "../../shell/registry";
import { Stub } from "../../shell/Stub";

export const id = "looks";
export const title = "Looks";
export const order = 4;
export const issues: Issue[] = [
  { number: 58, title: "Programmer: touch it, see it" },
  { number: 64, title: "Looks: store what you see" },
  { number: 63, title: "Palettes that stay linked" },
  { number: 59, title: "Effects across a group" },
  { number: 69, title: "Chasers and sequences" },
  { number: 60, title: "Pixel matrix effects" },
  { number: 71, title: "Collections" },
];

export function Screen() {
  return (
    <Stub
      areas={`"library programmer palettes" "library editor palettes"`}
      columns="minmax(0, 1fr) minmax(0, 2.4fr) minmax(0, 1fr)"
      rows="1fr auto"
      regions={[
        { id: "library", area: "library", title: "Library", line: "Everything that can play, with a type filter and search: looks, chases, effects, matrix effects, groups of looks." },
        { id: "programmer", area: "programmer", title: "Programmer", line: "The rig: tap fixtures to select them, then change Intensity, Colour, Position and Beam beside it. The rig follows at once." },
        { id: "editor", area: "editor", title: "Editor", line: "The editor for the selected type: steps for a chase, shape and spread for an effect, a pixel preview for a matrix." },
        { id: "palettes", area: "palettes", title: "Palettes", line: "Colours, positions and beams to apply with one tap." },
      ]}
    />
  );
}
