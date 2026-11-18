// What the engine reported last, kept in step with its replies. A calculation.
import type { ChannelLevel, Levels, Reply, State } from "./engine-api";

export interface Picture {
  state: State;
  universes: Levels[];
}

/** The picture after a reply. `undefined` until the first `Picture`. */
export function receive(picture: Picture | undefined, reply: Reply): Picture | undefined {
  if (reply === "Accepted" || "Refused" in reply) return picture;
  if ("Picture" in reply) return reply.Picture;
  if (picture === undefined) return undefined;
  return { state: reply.Changed.state, universes: merge(picture.universes, reply.Changed.channels) };
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
