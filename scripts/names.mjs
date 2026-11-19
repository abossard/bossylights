// The engine knows "an output", never a protocol or a device. Only the list of kinds
// may name one. See README, "The engine knows an output".

/** Folders checked, and the files in them that may name protocols and devices. */
export const CHECKED = ["crates/engine-core", "crates/engine"];
export const KINDS = "crates/engine-core/src/kinds.rs"; // the list of kinds
export const ALLOWED = [
  KINDS,
  "crates/engine/clippy.toml", // the ban list names the std socket types it bans
];

/** Ways out that are no kind of their own, banned with every kind. Not "serialize". */
const TRANSPORTS = ["udp", "serial"];

/** The variants of `enum OutputKind` in the source of the list of kinds, e.g. ["Ddp", "DmxUsbPro"]. */
export function kindsOf(source) {
  const start = source.search(/\benum\s+OutputKind\s*\{/);
  if (start === -1) return [];
  const body = source.slice(source.indexOf("{", start) + 1).replace(/\/\/.*$/gm, "");
  const kinds = [];
  let depth = 0;
  for (const token of body.matchAll(/[{}()[\]]|#\[[^\]]*\]|\b[A-Za-z_]\w*\b/g)) {
    const t = token[0];
    if ("{([".includes(t)) depth += 1;
    else if ("})]".includes(t)) depth -= 1;
    else if (depth === 0 && /^[A-Z]/.test(t)) kinds.push(t);
    if (depth < 0) break;
  }
  return kinds;
}

/** The words of a text, lowercase, split at spaces, `_`, `-` and camel case: "DmxUsbPro" is dmx usb pro. */
export function wordsOf(text) {
  return [...text.matchAll(/[A-Z]+(?![a-z])|[A-Z]?[a-z]+|\d+/g)].map((m) => m[0].toLowerCase());
}

/** Whether `words` holds `name` (a run of words) in a row. */
const holds = (words, name) => words.some((_, at) => name.every((word, i) => words[at + i] === word));

/**
 * Every line of `source` that names a protocol or device: "path:line: text". `kinds`:
 * the variants of `OutputKind`. A line names one when it spells a kind or a transport in
 * any case or separator, or names any variant of `OutputKind`.
 */
export function protocolNames(path, source, kinds) {
  if (ALLOWED.includes(path)) return [];
  const names = [...kinds, ...TRANSPORTS].map(wordsOf);
  return source.split("\n").flatMap((line, at) => {
    const words = wordsOf(line);
    const named = /\bOutputKind\s*::\s*[A-Z]/.test(line) || names.some((name) => holds(words, name));
    return named ? [`${path}:${at + 1}: ${line.trim()}`] : [];
  });
}
