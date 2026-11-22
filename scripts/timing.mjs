// CI judges what is sent, never how fast. A test that compares an elapsed time with a
// bound would fail on a noisy runner. Allowed: a literal bound of a second or more, or a
// bound that is not a literal on a line marked `// guard` (a hang guard of seconds).

/** Folders whose tests the check reads. */
export const CHECKED = ["crates", "e2e", "shell"];

const SECOND_MS = 1000;
const COMPARE = /[<>]=?|\bassert_(?:eq|ne)!|toBeLessThan|toBeGreaterThan/;
const ELAPSED = /\.elapsed\(\)|\.duration_since\(|Instant::now\(\)\s*-|Date\.now\(\)|performance\.now\(\)/;

/** The bound in milliseconds when the line writes one, else undefined. */
function boundMs(line) {
  const from = line.match(/Duration::from_(millis|micros|nanos|secs_f64|secs_f32|secs)\(\s*([\d._]+)/);
  if (from) {
    const n = Number(from[2].replaceAll("_", ""));
    const unit = { millis: 1, micros: 1e-3, nanos: 1e-6, secs_f64: 1000, secs_f32: 1000, secs: 1000 }[from[1]];
    return n * unit;
  }
  const as = line.match(/as_(millis|micros|nanos)\(\)\s*[<>]=?\s*([\d._]+)/);
  if (as) return Number(as[2].replaceAll("_", "")) * { millis: 1, micros: 1e-3, nanos: 1e-6 }[as[1]];
  const js = line.match(/toBe(?:LessThan|GreaterThan)\(\s*([\d._]+)/);
  return js ? Number(js[1].replaceAll("_", "")) : undefined;
}

const WAIT = /\bwaitForTimeout\(/;
const SLEEP = /\bsleep\(\s*Duration::from_(millis|micros|nanos|secs_f64|secs_f32)\(\s*([\d._]+)/;
const SLEEP_CONSTANT = /\bsleep\(\s*[A-Z][A-Z0-9_]*\s*\)/; // a named time: its size is not known here
const GUARD = /\/\/\s*guard\b/; // a hang guard of seconds, on a bound that is not a literal
const POLL = /\/\/\s*poll\b/;

/** True when the statement sleeps under a second, or waits for a fixed time, and is not marked `// poll`. */
function fixedWait(statement) {
  if (POLL.test(statement)) return false;
  if (WAIT.test(statement) || SLEEP_CONSTANT.test(statement)) return true;
  const sleep = statement.match(SLEEP);
  if (!sleep) return false;
  const n = Number(sleep[2].replaceAll("_", ""));
  const unit = { millis: 1, micros: 1e-3, nanos: 1e-6, secs_f64: 1000, secs_f32: 1000 }[sleep[1]];
  return n * unit < SECOND_MS;
}

/** The statements of a source with the line each starts on: lines are joined up to one that ends in `;`, `{`, or `}`. */
function statements(source) {
  const found = [];
  let text = "";
  let from = 0;
  source.split("\n").forEach((line, at) => {
    if (text === "") from = at + 1;
    text += `${line}\n`;
    if (/[;{}]\s*(?:\/\/.*)?$/.test(line)) {
      found.push({ at: from, text });
      text = "";
    }
  });
  if (text.trim() !== "") found.push({ at: from, text });
  return found;
}

/**
 * Every statement of a test file that compares an elapsed time with anything but a literal of a second or more (or a `// guard`),
 * or sleeps or waits a fixed time under a second without the `// poll` mark: "path:line: text".
 * A wait that polls a condition is marked `// poll` and is guarded by seconds.
 */
export function timingBounds(path, source) {
  return statements(source).flatMap(({ at, text }) => {
    const bound = boundMs(text);
    const safe = (bound !== undefined && bound >= SECOND_MS) || GUARD.test(text);
    const compares = ELAPSED.test(text) && COMPARE.test(text) && !safe;
    const first = text.split("\n").find((line) => line.trim() !== "") ?? "";
    return compares || fixedWait(text) ? [`${path}:${at + text.split("\n").findIndex((l) => l.trim() !== "")}: ${first.trim()}`] : [];
  });
}

/** The test code of a file: all of a tests or e2e file, the `#[cfg(test)]` module of a source file. Earlier lines are blanked so line numbers stay. */
export function testSource(path, source) {
  if (/(^|\/)(tests|e2e)\//.test(path)) return source;
  const at = source.search(/^#\[cfg\(test\)\]/m);
  return at === -1 ? "" : source.slice(0, at).replace(/[^\n]/g, "") + source.slice(at);
}

// Files the check reads: Rust and TypeScript sources, whose test code is picked by testSource.
export const isTestFile = (path) => /\.(rs|ts|mjs)$/.test(path);
