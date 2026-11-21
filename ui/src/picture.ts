// What the engine reported last, kept in step with its replies. A calculation.
import type { ChannelLevel, Levels, Library, Output, Project, ProjectOutput, Reply, State, Status } from "./engine-api";

export interface Picture {
  state: State;
  project: Project;
  library: Library;
  universes: Levels[];
}

/**
 * A reply as the engine sends it: a unit variant as a string, any other as an object of exactly one key
 * (serde's externally tagged form). `Picture` and `Changed` also carry the parts the picture is made of.
 * Anything else is undefined: the engine said something this UI cannot read.
 */
export function decode(text: string): Reply | undefined {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return undefined;
  }
  if (typeof value === "string") return value as Reply;
  if (!isObject(value)) return undefined;
  const keys = Object.keys(value);
  if (keys.length !== 1) return undefined;
  const body = value[keys[0] ?? ""];
  if (keys[0] === "Picture" && !(isParts(body) && Array.isArray(body.universes) && body.universes.every(Array.isArray))) return undefined;
  if (keys[0] === "Changed" && !(isParts(body) && Array.isArray(body.channels) && body.channels.every(isChannel))) return undefined;
  return value as Reply;
}

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** The parts every picture has: a state with its outputs, a project with its outputs, a library. */
const isParts = (value: unknown): value is Record<string, unknown> =>
  isObject(value) &&
  isObject(value.state) &&
  Array.isArray(value.state.outputs) &&
  isObject(value.project) &&
  Array.isArray(value.project.outputs) &&
  value.library !== undefined;

/** A channel at its level: universe, address and value as numbers. */
const isChannel = (value: unknown): boolean =>
  isObject(value) && typeof value.universe === "number" && typeof value.address === "number" && typeof value.value === "number";

/** The picture after a reply. `undefined` until the first `Picture`. Answers to a request change nothing. */
export function receive(picture: Picture | undefined, reply: Reply): Picture | undefined {
  if (typeof reply === "string") return picture;
  if ("Picture" in reply) return reply.Picture;
  if (!("Changed" in reply) || picture === undefined) return picture;
  const { state, project, library, channels } = reply.Changed;
  return { state, project, library, universes: merge(picture.universes, channels) };
}

/** The universes with these channels at their new levels. Counts from 1, as the engine does. */
export function merge(universes: Levels[], channels: ChannelLevel[]): Levels[] {
  if (channels.length === 0) return universes;
  const next = universes.map((levels) => [...levels]);
  for (const { universe, address, value } of channels) {
    const levels = next[universe - 1];
    if (levels !== undefined && address >= 1 && address <= levels.length) levels[address - 1] = value;
  }
  return next;
}

/** Whether the picture is what the engine says now, or the last one before the connection went. */
export type Seen = "now" | "before";

/** Every output the engine runs, in plain words: where it sends, its state, and how often while it runs. */
export function outputLines(picture: Picture, seen: Seen): string[] {
  return picture.state.outputs.flatMap((output, at) => (output ? [outputLine(output, picture.project.outputs[at], seen)] : []));
}

/** "DMX USB Pro, running, 44 a second", "DDP to 10.0.0.5:4048, reconnecting". Seen before: "state not known". */
export function outputLine(output: Output, setUp: ProjectOutput | null | undefined, seen: Seen): string {
  if (seen === "before") return `${where(setUp)}, state not known`;
  const state = stateWord(output.status);
  return [where(setUp), state, ...(state === "running" ? [`${output.rate} a second`] : [])].join(", ");
}

function where(setUp: ProjectOutput | null | undefined): string {
  if (!setUp) return "an output";
  if ("Attached" in setUp) return "a test device";
  const kind = setUp.Kind.kind;
  if ("Ddp" in kind) return `DDP to ${kind.Ddp.address}`;
  return kind.DmxUsbPro.adapter === 1 ? "DMX USB Pro" : `DMX USB Pro ${kind.DmxUsbPro.adapter}`;
}

function stateWord(status: Status): string {
  if (typeof status !== "string") return "reconnecting";
  switch (status) {
    case "NotFound":
      return "not found";
    case "Connecting":
      return "connecting";
    case "Running":
      return "running";
  }
}
