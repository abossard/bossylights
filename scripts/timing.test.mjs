import assert from "node:assert/strict";
import { test } from "node:test";
import { isTestFile, testSource, timingBounds } from "./timing.mjs";

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
  ["ours.set_timeout(Duration::from_millis(20))?;", false],
  ["await page.waitForTimeout(500);", true],
  ["thread::sleep(Duration::from_millis(250));", true],
  ["tokio::time::sleep(Duration::from_millis(100)).await;", true],
  ["thread::sleep(Duration::from_secs_f64(0.5));", true],
  ["thread::sleep(Duration::from_millis(1)); // poll", false],
  ["assert!(s.elapsed() < BOUND);", true],
  ["assert!(s.elapsed() < Duration::from_millis(frames * 10));", true],
  ["assert!(s.elapsed() < Duration::from_secs(limit));", true],
  ["assert!(s.elapsed() < guard);", true],
  ["assert!(s.elapsed() < guard); // guard", false],
  ["expect(Date.now() - started).toBeLessThan(limit);", true],
  ["assert!(s.elapsed() < Duration::from_secs(5));", false],
  ["assert!(s.elapsed() > Duration::from_secs(1));", false],
  ["thread::sleep(LINK_TIME);", true],
  ["thread::sleep(LINK_TIME); // poll", false],
  ["thread::sleep(Duration::from_secs(3));", false],
  ["let started = Instant::now();", false],
  ["assert!(Instant::now().duration_since(started) < Duration::from_millis(50));", true],
  ["assert!(Instant::now().duration_since(started) < Duration::from_secs(2));", false],
  ["assert!(Instant::now() - started < Duration::from_millis(5));", true],
  ["assert!(Instant::now() - started < Duration::from_secs(2));", false],
  ["assert_eq!(started.elapsed(), Duration::from_millis(5));", true],
  ["assert_ne!(started.elapsed(), Duration::from_millis(5));", true],
  ["assert_eq!(frames, 5);", false],
  ["expect(Date.now() - started).toBeGreaterThan(200);", true],
  ["expect(Date.now() - started).toBeGreaterThan(2000);", false],
];
for (const [line, flagged] of cases) {
  test(`${flagged ? "flags" : "allows"}: ${line}`, () => {
    assert.equal(timingBounds(FILE, line).length, flagged ? 1 : 0);
  });
}

test("the hit says where", () => {
  const source = "a;\nassert!(s.elapsed() < Duration::from_millis(5));";
  assert.deepEqual(timingBounds(FILE, source), [`${FILE}:2: assert!(s.elapsed() < Duration::from_millis(5));`]);
});

// [source, number of hits]: a check reads a statement, not a line
const statementCases = [
  ["assert!(\n    started.elapsed()\n        < Duration::from_millis(5),\n    \"late\"\n);", 1],
  ["let late = started.elapsed()\n    < Duration::from_millis(5);", 1],
  ["assert!(\n    started.elapsed()\n        < Duration::from_secs(10),\n);", 0],
  ["let late = started.elapsed()\n    < BOUND;", 1],
  ["assert!(\n    started.elapsed() < guard, // guard\n);", 0],
  ["let t = Instant::now();\nassert!(frames > 10);\nlet d = Duration::from_millis(5);", 0],
];
for (const [source, hits] of statementCases) {
  test(`statements: ${JSON.stringify(source)}`, () => assert.equal(timingBounds(FILE, source).length, hits));
}

// [path, is a file the check reads]
const fileCases = [
  ["crates/engine/tests/tick.rs", true],
  ["e2e/counter.spec.ts", true],
  ["crates/engine/src/lib.rs", true],
  ["shell/src/main.rs", true],
  ["crates/engine/tests/mock/mod.rs", true],
  ["README.md", false],
  ["crates/engine/Cargo.toml", false],
];
for (const [path, expected] of fileCases) {
  test(`reads ${path}: ${expected}`, () => assert.equal(isTestFile(path), expected));
}

// [path, source, the test code, earlier lines blanked so line numbers stay]
const sourceCases = [
  ["crates/engine/tests/a.rs", "fn a() {}\n", "fn a() {}\n"],
  ["e2e/a.spec.ts", "x\n", "x\n"],
  ["crates/engine/src/lib.rs", "thread::sleep(a);\n", ""],
  ["crates/engine/src/lib.rs", "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n", "\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n"],
];
for (const [path, source, expected] of sourceCases) {
  test(`test source of ${path}: ${JSON.stringify(source)}`, () => assert.equal(testSource(path, source), expected));
}
