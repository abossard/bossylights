import assert from "node:assert/strict";
import { test } from "node:test";
import { timingBounds } from "./timing.mjs";

const FILE = "crates/engine/tests/tick.rs";

// [line, flagged]
const cases = [
  ["assert!(started.elapsed() < Duration::from_millis(100));", true],
  ["assert!(start.elapsed() <= Duration::from_micros(500), \"late\");", true],
  ["assert!(started.elapsed() < Duration::from_secs_f64(0.5));", true],
  ["assert!(Duration::from_millis(100) > started.elapsed());", true],
  ["assert!(t.elapsed().as_millis() < 50);", true],
  ["expect(Date.now() - started).toBeLessThan(200);", true],
  ["assert!(started.elapsed() < Duration::from_secs(10), \"no frames\");", false],
  ["assert!(started.elapsed() < Duration::from_millis(2000));", false],
  ["thread::sleep(Duration::from_millis(100));", false],
  ["ours.set_timeout(Duration::from_millis(20))?;", false],
  ["await page.waitForTimeout(500);", false],
  ["let started = Instant::now();", false],
];
for (const [line, flagged] of cases) {
  test(`${flagged ? "flags" : "allows"}: ${line}`, () => {
    assert.equal(timingBounds(FILE, line).length, flagged ? 1 : 0);
  });
}

test("the hit says where", () => {
  const source = "a\nassert!(s.elapsed() < Duration::from_millis(5));";
  assert.deepEqual(timingBounds(FILE, source), [`${FILE}:2: assert!(s.elapsed() < Duration::from_millis(5));`]);
});
