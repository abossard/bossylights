// What only a debug build of the shell may hold: the test hooks of shell/src/main.rs.
// A release build never reads another page, runs a test script or lets a page close the window.

/** Names of the debug-only test hooks, as they stand in the program's bytes. */
export const DEBUG_ONLY = ["BOSSYLIGHTS_UI", "BOSSYLIGHTS_WINDOW_SCRIPT", "test-page-closes-the-window"];

/** The names of `names` found in a program's bytes (read as latin1). */
export function found(bytes, names) {
  return names.filter((name) => bytes.includes(name));
}

/** What is wrong: a debug-only name in the release program, or one missing from the debug program (the list is stale). */
export function releaseShellViolations(release, debug, names) {
  return [
    ...found(release, names).map((name) => `the release shell holds the debug-only ${name}`),
    ...names.filter((name) => !debug.includes(name)).map((name) => `the debug shell has no ${name}: update DEBUG_ONLY`),
  ];
}

/** The file of the machine's engine slot (crates/engine-slot, #319): only a test build may name it. */
export const SLOT = "bossylights-engine.slot";

/** What is wrong: the release engine names the slot (it would wait for a test's engine), or the slot's source no longer names SLOT (stale). */
export function releaseEngineViolations(release, slotSource) {
  return [
    ...found(release, [SLOT]).map((name) => `the release engine holds the test-only ${name}`),
    ...(slotSource.includes(`"${SLOT}"`) ? [] : [`crates/engine-slot names no ${SLOT}: update SLOT`]),
  ];
}
