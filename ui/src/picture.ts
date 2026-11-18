// What the engine reported last, kept in step with its replies. A calculation.
import type { ChannelLevel, Levels, Library, Reply, State } from "./engine-api";

export interface Picture {
  state: State;
  library: Library;
  universes: Levels[];
}

/** The picture after a reply. `undefined` until the first `Picture`. Answers to a request change nothing. */
export function receive(picture: Picture | undefined, reply: Reply): Picture | undefined {
  if (typeof reply === "string") return picture;
  if ("Picture" in reply) return reply.Picture;
  if (!("Changed" in reply) || picture === undefined) return picture;
  const { state, library, channels } = reply.Changed;
  return { state, library, universes: merge(picture.universes, channels) };
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
