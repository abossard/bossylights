// The real shell and engine processes: the engine outlives the window (#107).
// Its own free port, so it never meets the engine of `npm run dev`, the other tests or another run.
import { expect, test } from "@playwright/test";
import { type ChildProcess, execFileSync, spawn, spawnSync } from "node:child_process";
import { createSocket } from "node:dgram";
import { createServer, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import type { Command, OutputKind } from "../ui/src/engine-api.ts";
import { cargoBuild } from "../scripts/cargo-build.mjs";
import { query, request, send } from "./api";
import { NOWHERE, SHELL_ENGINE } from "./ports";

const ADDRESS = SHELL_ENGINE;
/** Where cargo put the shell and the engine: filled before the tests. */
let programs: Record<string, string> = {};
const WAIT = { timeout: 30_000 };

interface Shell {
  process: ChildProcess;
  said: () => string;
}

/** Every program a test started. A test that times out never reaches its `finally`, and its worker ends without them: `afterEach` stops them, so none meets the next test (#317). */
const started = new Set<ChildProcess>();

/** Starts the shell as the tests do: its window hidden, the app never in front (#316). `{ BOSSYLIGHTS_HIDDEN_WINDOW: undefined }` starts it as a person does. */
function startShell(address = ADDRESS, env: Record<string, string | undefined> = {}): Shell {
  const child = spawn(programs.bossylights, [], {
    env: { ...process.env, BOSSYLIGHTS_ENGINE: address, BOSSYLIGHTS_HIDDEN_WINDOW: "1", ...env },
  });
  started.add(child);
  let text = "";
  child.stdout?.on("data", (chunk: Buffer) => (text += chunk.toString()));
  child.stderr?.on("data", (chunk: Buffer) => (text += chunk.toString()));
  child.on("error", (error) => (text += `the shell did not start: ${error.message}\n`)); // a failed test, not a crashed worker
  return { process: child, said: () => text };
}

async function kill(child: ChildProcess) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const exited = new Promise((done) => child.once("exit", done));
  child.kill("SIGKILL");
  await exited;
}

/** No engine process for this address is left. The engine answers no more before it ends, and keeps the port's lock until it has: a new engine started too early is refused. */
const gone = () => {
  try {
    execFileSync("pgrep", ["-f", `bossylights-engine --listen ${ADDRESS}`], { stdio: "ignore" });
    return false;
  } catch {
    return true; // pgrep exits 1 when it finds none
  }
};

const health = () =>
  fetch(`http://${ADDRESS}/health`).then(
    (response) => response.status,
    () => "nobody",
  );

test.beforeAll(() => {
  test.setTimeout(600_000); // the first run compiles the shell
  programs = cargoBuild(["-q", "-p", "shell", "-p", "engine-server"]);
});

// However the test ended: no window and no engine on the test port are left.
test.afterEach(async () => {
  for (const child of started) await kill(child);
  started.clear();
  if ((await health()) === 200) expect(await request(ADDRESS, "Stop")).toBe("Stopping");
  await expect.poll(health, WAIT).toBe("nobody");
  await expect.poll(gone, WAIT).toBe(true);
});

test("kill the shell: the engine keeps sending, a new shell adopts it and the frame count goes on", async () => {
  test.setTimeout(120_000);
  const udp = createSocket("udp4");
  let packets = 0;
  udp.on("message", () => packets++);
  await new Promise<void>((bound) => udp.bind(0, "127.0.0.1", bound));
  const first = startShell();
  try {
    await expect.poll(first.said, WAIT).toContain(`started the engine on ${ADDRESS}`);
    await expect.poll(first.said, WAIT).toContain(`the window talks to the engine on ${ADDRESS}`);
    await expect.poll(health, WAIT).toBe(200);
    const kind: OutputKind = { Ddp: { address: `127.0.0.1:${udp.address().port}`, color: "Rgb" } };
    const command: Command = { SetOutput: { output: 16, kind, universes: { first: 1, count: 1 }, rate: "30" } };
    expect(await send(ADDRESS, command)).toBe("Accepted");
    const before = (await query(ADDRESS)).state.frame;

    await kill(first.process);
    packets = 0;
    await expect.poll(() => packets, WAIT).toBeGreaterThan(30); // still sending, with no shell
    expect(await health()).toBe(200);

    const second = startShell();
    await expect.poll(second.said, WAIT).toContain(`adopted the engine on ${ADDRESS}`);
    const state = (await query(ADDRESS)).state;
    expect(state.frame).toBeGreaterThan(before);
    expect(state.outputs[15]?.rate).toBe("30"); // the same engine: it still has the output
  } finally {
    udp.close();
  }
});

/** A page for the window that closes the window, as a person does, once `close` is called. The page asks, the answer
 * waits for `close`, and a dropped ask is asked again: a hidden window's timers are slowed, so the page uses none. */
async function closingPage() {
  let closing = false;
  let heard = false;
  const asked: ServerResponse[] = [];
  const server = createServer((ask, answer) => {
    answer.setHeader("access-control-allow-origin", "*");
    if (ask.url !== "/close") return answer.end("<!doctype html><title>Bossylights</title><p>closing page</p>");
    heard = true;
    if (closing) answer.end("yes");
    else asked.push(answer);
  });
  await new Promise<void>((listening) => server.listen(0, "127.0.0.1", listening));
  const page = `http://127.0.0.1:${(server.address() as AddressInfo).port}/`;
  return {
    page,
    /** The page has asked: it loaded and its script runs. */
    heard: () => heard,
    script: `const ask = () => fetch("${page}close").then((r) => r.text()).then((said) => {
      if (said === "yes") window.__TAURI_INTERNALS__.invoke("plugin:window|close", { label: "main" });
      else ask();
    }, ask);
    ask();`,
    close: () => {
      closing = true;
      for (const answer of asked.splice(0)) answer.end("yes");
    },
    stop: () => {
      server.closeAllConnections();
      return new Promise((closed) => server.close(closed));
    },
  };
}

// [whose engine, how it is there before the window]. Theory idea 9: a closed window cannot stop the lights.
const engines = [
  ["started by the shell", false],
  ["adopted", true],
] as const;
for (const [whose, before] of engines) {
  test(`close the window: the engine ${whose} keeps sending its DDP bytes`, async () => {
    test.setTimeout(120_000);
    const udp = createSocket("udp4");
    const packets: Buffer[] = [];
    udp.on("message", (packet) => packets.push(packet));
    await new Promise<void>((bound) => udp.bind(0, "127.0.0.1", bound));
    if (before) started.add(spawn(programs["bossylights-engine"], ["--listen", ADDRESS], { stdio: "ignore", detached: true }));
    if (before) await expect.poll(health, WAIT).toBe(200);
    const window = await closingPage();
    const shell = startShell(ADDRESS, { BOSSYLIGHTS_UI: window.page, BOSSYLIGHTS_WINDOW_SCRIPT: window.script });
    try {
      await expect.poll(shell.said, WAIT).toContain(`${before ? "adopted" : "started"} the engine on ${ADDRESS}`);
      await expect.poll(health, WAIT).toBe(200);
      const kind: OutputKind = { Ddp: { address: `127.0.0.1:${udp.address().port}`, color: "Rgb" } };
      const output: Command = { SetOutput: { output: 16, kind, universes: { first: 1, count: 1 }, rate: "30" } };
      expect(await send(ADDRESS, output)).toBe("Accepted");
      expect(await send(ADDRESS, { SetChannel: { channel: { universe: 1, address: 1 }, value: 77 } })).toBe("Accepted");

      window.close();
      await expect.poll(window.heard, { ...WAIT, message: "the window's page asks whether to close" }).toBe(true);
      await expect
        .poll(() => shell.process.exitCode, { ...WAIT, message: "the app ends after its last window closed" })
        .toBe(0);
      packets.length = 0;
      // DDP (3waylabs): a 10 byte header, then channel 1 of universe 1.
      await expect.poll(() => packets.length, WAIT).toBeGreaterThan(30);
      expect(packets.at(-1)?.[10]).toBe(77);
      expect(await health()).toBe(200);
    } finally {
      await kill(shell.process);
      await window.stop();
      udp.close();
    }
  });
}

/** What the window's page shows, as the text its layout makes, and whether it is visible and focused: the page posts it
 * here on every change. A hidden window's timers are slowed, so the page uses none. The newest post wins. */
async function windowText() {
  let posts = 0;
  let last = new URLSearchParams({ post: "0" });
  let seen = "";
  const server = createServer((ask, answer) => {
    let text = "";
    ask.on("data", (chunk: Buffer) => (text += chunk.toString()));
    ask.on("end", () => {
      posts++;
      const said = new URL(ask.url ?? "/", to).searchParams;
      if (Number(said.get("post")) > Number(last.get("post"))) [last, seen] = [said, text];
      answer.end();
    });
  });
  await new Promise<void>((listening) => server.listen(0, "127.0.0.1", listening));
  const to = `http://127.0.0.1:${(server.address() as AddressInfo).port}/`;
  return {
    /** Runs in the window before its page, and on: posts what the page shows. */
    script: `{
      let post = 0;
      const send = () => {
        const page = new URLSearchParams({
          post: String(++post),
          readyState: document.readyState,
          href: location.href,
          visibility: document.visibilityState,
          focus: String(document.hasFocus()),
        });
        fetch("${to}?" + page, { method: "POST", mode: "no-cors", body: document.body?.innerText ?? "" }).catch(() => {});
      };
      new MutationObserver(send).observe(document, { subtree: true, childList: true, characterData: true });
      for (const change of ["readystatechange", "visibilitychange"]) document.addEventListener(change, send);
      for (const change of ["focus", "blur"]) window.addEventListener(change, send);
      send();
    }`,
    /** The lines the page showed last, in lower case (the status is styled upper case). */
    lines: () => seen.toLowerCase().split("\n"),
    /** How the window showed last: page visibility and focus, as the page said them. */
    showing: () => ({ visibility: last.get("visibility"), focus: last.get("focus") }),
    status: () =>
      `Window posts: ${posts}\nLast readyState: ${last.get("readyState") ?? "(none)"}\nLast href: ${last.get("href") ?? "(none)"}`,
    close: () => new Promise((closed) => server.close(closed)),
  };
}

function windowProcesses(): string {
  const ps = spawnSync("ps", ["-eo", "pid,ppid,stat,etime,args"], { encoding: "utf8" });
  if (ps.status !== 0) return ps.error?.message || ps.stderr?.trim() || `ps exited with ${ps.signal ?? ps.status}`;
  return ps.stdout.split("\n").filter((line) => /WebKit|bwrap|bossylights/.test(line)).join("\n") || "(none)";
}

// The page's text as laid out in the window's web view is read, not its pixels.
test("the window shows the engine on the test port", async ({ baseURL }) => {
  test.setTimeout(120_000);
  const window = await windowText();
  const shell = startShell(ADDRESS, { BOSSYLIGHTS_UI: baseURL!, BOSSYLIGHTS_WINDOW_SCRIPT: window.script });
  try {
    await expect.poll(window.lines, WAIT).toContain("connected");
    expect(window.lines()).toContain(`engine on ${ADDRESS}`);
  } catch (cause) {
    throw new Error(
      `Shell output:\n${shell.said() || "(none)"}\nLast window lines: ${JSON.stringify(window.lines())}\n` +
        `${window.status()}\nProcesses (pid, ppid, stat, etime, args):\n${windowProcesses()}`,
      { cause },
    );
  } finally {
    await kill(shell.process);
    await window.close();
  }
});

test("a second shell for the same engine opens no window and stops nothing", async () => {
  test.setTimeout(120_000);
  const first = startShell();
  await expect.poll(first.said, WAIT).toContain("the window talks to");
  await expect.poll(health, WAIT).toBe(200); // the engine the first shell started is up
  const second = startShell();
  await expect.poll(second.said, WAIT).toContain("a window for the engine on");
  await expect.poll(() => second.process.exitCode, WAIT).toBe(0);
  expect(second.said()).not.toContain("the window talks to");
  expect(await health()).toBe(200);
});

test("another engine version on the port: the shell says so and starts nothing", async () => {
  test.setTimeout(120_000);
  const [host, port] = ADDRESS.split(":");
  const other = createServer((ask, answer) => {
    if (ask.url === "/version") answer.end("0");
    else answer.writeHead(404).end();
  });
  await new Promise<void>((listening) => other.listen(Number(port), host, listening));
  const shell = startShell();
  try {
    await expect.poll(shell.said, WAIT).toContain(`the engine on ${ADDRESS} speaks api 0`);
    expect(shell.said()).toContain("starting nothing");
    expect(shell.said()).not.toContain("started the engine");
  } finally {
    await kill(shell.process);
    await new Promise((closed) => other.close(closed));
  }
});

test("an engine of its own that ends at start: the shell does not say the window talks to it", async () => {
  test.setTimeout(120_000);
  // An address of no machine here (TEST-NET-1): nothing answers on it, and the engine cannot listen on it.
  const nowhere = NOWHERE;
  const shell = startShell(nowhere);
  await expect.poll(shell.said, WAIT).toContain(`the window has no engine on ${nowhere}`);
  expect(shell.said()).not.toContain("the window talks to");
});

/** How macOS shows the app of `pid`: its activation policy ("0" regular, with a Dock icon; "1" accessory, without), whether it is the frontmost app, and how many of its windows are on screen. */
function macApp(pid: number) {
  const script = `ObjC.import("AppKit"); ObjC.import("CoreGraphics");
  function run(argv) {
    const pid = Number(argv[0]);
    const app = $.NSRunningApplication.runningApplicationWithProcessIdentifier(pid);
    const windows = ObjC.deepUnwrap(ObjC.castRefToObject($.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly, 0)));
    return JSON.stringify({
      policy: String(app.activationPolicy),
      frontmost: $.NSWorkspace.sharedWorkspace.frontmostApplication.processIdentifier === pid,
      onScreen: windows.filter((w) => w.kCGWindowOwnerPID === pid).length,
    });
  }`;
  return JSON.parse(execFileSync("osascript", ["-l", "JavaScript", "-e", script, String(pid)], { encoding: "utf8" })) as {
    policy: string;
    frontmost: boolean;
    onScreen: number;
  };
}

// [who starts the window, the hook, what the page says, what macOS says]. #316: the tests' windows stay out of the owner's way.
const starts = [
  ["the tests start", "1", { visibility: "hidden", focus: "false" }, { policy: "1", frontmost: false, onScreen: 0 }],
  ["a person starts", undefined, { visibility: "visible", focus: "true" }, { policy: "0", frontmost: true, onScreen: 1 }],
] as const;
for (const [who, hook, page, mac] of starts) {
  test(`a window ${who}: ${page.visibility}, focus ${page.focus}`, async ({ baseURL }) => {
    // A person's start takes the focus: on the owner's Mac that is what #316 removes, so it runs in CI only.
    test.skip(hook === undefined && !process.env.CI, "it would take the focus from the person at this computer");
    test.setTimeout(120_000);
    const window = await windowText();
    // No engine: what the window shows is all this test reads.
    const shell = startShell(NOWHERE, { BOSSYLIGHTS_UI: baseURL!, BOSSYLIGHTS_WINDOW_SCRIPT: window.script, BOSSYLIGHTS_HIDDEN_WINDOW: hook });
    try {
      // Linux CI runs on a display with no window manager: no window there gets the focus.
      const seen = hook === undefined && process.platform === "linux" ? { visibility: page.visibility } : page;
      await expect.poll(window.showing, WAIT).toMatchObject(seen);
      if (process.platform === "darwin") expect(macApp(shell.process.pid!)).toEqual(mac);
    } catch (cause) {
      throw new Error(`Shell output:\n${shell.said() || "(none)"}\n${window.status()}`, { cause });
    } finally {
      await kill(shell.process);
      await window.close();
    }
  });
}

// #324: WebKit suspends a hidden window's page soon after it loads. A test window's page must run on, or a page that
// has not loaded by then never loads, and a test that waits for it waits for nothing.
test("a window the tests start: its hidden page keeps running", async () => {
  test.setTimeout(120_000);
  let first = 0;
  let latest = 0;
  // Each ask is answered a second later, by this process: the hidden page itself uses no timers.
  const server = createServer((ask, answer) => {
    latest = Date.now();
    first ||= latest;
    answer.setHeader("access-control-allow-origin", "*");
    if (ask.url !== "/ask") return answer.end("<!doctype html><title>Bossylights</title><p>asking page</p>");
    setTimeout(() => answer.end("again"), 1_000);
  });
  await new Promise<void>((listening) => server.listen(0, "127.0.0.1", listening));
  const page = `http://127.0.0.1:${(server.address() as AddressInfo).port}/`;
  const script = `const ask = () => fetch("${page}ask").then(ask, ask); ask();`;
  // No engine: the page is all this test reads.
  const shell = startShell(NOWHERE, { BOSSYLIGHTS_UI: page, BOSSYLIGHTS_WINDOW_SCRIPT: script });
  try {
    await expect.poll(() => first, { ...WAIT, message: "the window loads its page" }).toBeGreaterThan(0);
    await expect
      .poll(() => latest - first, { ...WAIT, message: "the hidden page still asks 20 s after it loaded" })
      .toBeGreaterThan(20_000);
  } finally {
    await kill(shell.process);
    server.closeAllConnections();
    await new Promise((closed) => server.close(closed));
  }
});
