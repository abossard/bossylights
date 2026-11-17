import type { Issue } from "../../shell/registry";
import { Stub } from "../../shell/Stub";

export const id = "patch";
export const title = "Patch";
export const order = 5;
export const issues: Issue[] = [
  { number: 50, title: "Patch: fixtures, modes, addresses" },
  { number: 54, title: "Guided patch" },
  { number: 41, title: "Fixture library: QLC+, OFL, GDTF" },
  { number: 56, title: "Groups and the rig map" },
];

export function Screen() {
  return (
    <Stub
      areas={`"list map details" "strip strip strip"`}
      columns="minmax(0, 1fr) minmax(0, 2fr) minmax(0, 1fr)"
      rows="1fr auto"
      regions={[
        { id: "fixtures", area: "list", title: "Fixtures", line: "Every light by name plate, model and mode second, address last. Add lights starts the guided flow here, in place." },
        { id: "map", area: "map", title: "Rig map", line: "Where the lights are. Drag fixtures, groups shown as outlines." },
        { id: "details", area: "details", title: "Details", line: "The selected fixture: model, mode, address, group." },
        { id: "addresses", area: "strip", title: "Addresses", line: "The 512 addresses of the universe with used ranges and conflicts." },
      ]}
    />
  );
}
