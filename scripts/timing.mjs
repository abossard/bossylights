// CI judges what is sent, never how fast. A test that compares an elapsed time with a
// bound under one second would fail on a noisy runner. Hang guards of seconds are fine.

/** Folders whose tests the check reads. */
export const CHECKED = ["crates", "e2e"];

const SECOND_MS = 1000;
const COMPARE = /[<>]=?|toBeLessThan|toBeGreaterThan/;
const ELAPSED = /\.elapsed\(\)|Date\.now\(\)|performance\.now\(\)/;

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

/** Every line of a test file that compares an elapsed time with a bound under a second: "path:line: text". */
export function timingBounds(path, source) {
  return source.split("\n").flatMap((line, at) => {
    const bound = boundMs(line);
    const hit = ELAPSED.test(line) && COMPARE.test(line) && bound !== undefined && bound < SECOND_MS;
    return hit ? [`${path}:${at + 1}: ${line.trim()}`] : [];
  });
}

// Test files: under a tests folder (crates) or e2e.
export const isTestFile = (path) => /(^|\/)(tests|e2e)\/.+\.(rs|ts|mjs)$/.test(path);
