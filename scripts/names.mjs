// The engine knows "an output", never a protocol or a device. Only the list of kinds
// may name one. See README, "The engine knows an output".

/** Folders checked, and the files in them that may name protocols and devices. */
export const CHECKED = ["crates/engine-core", "crates/engine"];
export const ALLOWED = [
  "crates/engine-core/src/kinds.rs", // the list of kinds
  "crates/engine/clippy.toml", // the ban list names the std socket types it bans
];

/** DDP, UDP, DMX USB Pro and serial, in any spelling code uses. Not "serialize". */
const NAME = /\b(ddp|udp)|dmx[ _-]?usb[ _-]?pro|\bserial(?!i[sz])/i;

/** Every line of `source` that names a protocol or device: "path:line: text". */
export function protocolNames(path, source) {
  if (ALLOWED.includes(path)) return [];
  return source
    .split("\n")
    .flatMap((line, at) => (NAME.test(line) ? [`${path}:${at + 1}: ${line.trim()}`] : []));
}
