// The real shell and engine processes: the engine outlives the window (#107).
// Its own free port, so it never meets the engine of `npm run dev`, the other tests or another run.
import { expect, test } from "@playwright/test";
import { type ChildProcess, execFileSync, spawn, spawnSync } from "node:child_process";
import { createSocket } from "node:dgram";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { NOWHERE, SHELL_ENGINE } from "./ports";

const ADDRESS = SHELL_ENGINE;
const SHELL = `target/debug/bossylights${process.platform === "win32" ? ".exe" : ""}`;
const WAIT = { timeout: 30_000 };

interface Shell {
  process: ChildProcess;
  said: () => string;
}

function startShell(address = ADDRESS, env: Record<string, string> = {}): Shell {
  const child = spawn(SHELL, [], { env: { ...process.env, BOSSYLIGHTS_ENGINE: address, ...env } });
  let text = "";
  child.stdout?.on("data", (chunk: Buffer) => (text += chunk.toString()));
  child.stderr?.on("data", (chunk: Buffer) => (text += chunk.toString()));
  return { process: child, said: () => text };
}

async function kill(shell: Shell) {
  if (shell.process.exitCode !== null || shell.process.signalCode !== null) return;
  const exited = new Promise((done) => shell.process.once("exit", done));
  shell.process.kill("SIGKILL");
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

/** The engine's first answer to a request that is not a `Changed`. */
async function request(message: unknown): Promise<any> {
  const socket = new WebSocket(`ws://${ADDRESS}/api`);
  await new Promise((opened, failed) => {
    socket.onopen = opened;
    socket.onerror = failed;
  });
  const reply = new Promise((answered) => {
    socket.onmessage = (event) => {
      const data = JSON.parse(String(event.data));
      if (data.Changed === undefined) answered(data);
    };
  });
  socket.send(JSON.stringify(message));
  const answer = await reply;
  socket.close();
  return answer;
}

const health = () =>
  fetch(`http://${ADDRESS}/health`).then(
    (response) => response.status,
    () => "nobody",
  );

test.beforeAll(() => {
  test.setTimeout(600_000); // the first run compiles the shell
  execFileSync("cargo", ["build", "-q", "-p", "shell", "-p", "engine-server"], { stdio: "inherit" });
});

test("kill the shell: the engine keeps sending, a new shell adopts it and the frame count goes on", async () => {
  test.setTimeout(120_000);
  const udp = createSocket("udp4");
  let packets = 0;
  udp.on("message", () => packets++);
  await new Promise<void>((bound) => udp.bind(0, "127.0.0.1", bound));
  const first = startShell();
  let second: Shell | undefined;
  try {
    await expect.poll(first.said, WAIT).toContain(`started the engine on ${ADDRESS}`);
    await expect.poll(first.said, WAIT).toContain(`the window talks to the engine on ${ADDRESS}`);
    await expect.poll(health, WAIT).toBe(200);
    const kind = { Ddp: { address: `127.0.0.1:${udp.address().port}`, color: "Rgb" } };
    const command = { SetOutput: { output: 16, kind, universes: { first: 1, count: 1 }, rate: "30" } };
    expect(await request({ Send: { command } })).toBe("Accepted");
    const before = (await request("Query")).Picture.state.frame;

    await kill(first);
    packets = 0;
    await expect.poll(() => packets, WAIT).toBeGreaterThan(30); // still sending, with no shell
    expect(await health()).toBe(200);

    second = startShell();
    await expect.poll(second.said, WAIT).toContain(`adopted the engine on ${ADDRESS}`);
    const state = (await request("Query")).Picture.state;
    expect(state.frame).toBeGreaterThan(before);
    expect(state.outputs[15]?.rate).toBe("30"); // the same engine: it still has the output
  } finally {
    await kill(first);
    if (second) await kill(second);
    if ((await health()) === 200) expect(await request("Stop")).toBe("Stopping");
    await expect.poll(health, WAIT).toBe("nobody");
    await expect.poll(gone, WAIT).toBe(true);
    udp.close();
  }
});

/** What the window's page shows, as the text its layout makes: the page posts it here. */
async function windowText() {
  let posts = 0;
  let last = new URLSearchParams();
  let seen = "";
  const server = createServer((ask, answer) => {
    let text = "";
    ask.on("data", (chunk: Buffer) => (text += chunk.toString()));
    ask.on("end", () => {
      posts++;
      last = new URL(ask.url ?? "/", to).searchParams;
      seen = text;
      answer.end();
    });
  });
  await new Promise<void>((listening) => server.listen(0, "127.0.0.1", listening));
  const to = `http://127.0.0.1:${(server.address() as AddressInfo).port}/`;
  return {
    /** Runs in the window before its page, and on: posts what the page shows. */
    script: `setInterval(() => {
      const page = new URLSearchParams({ readyState: document.readyState, href: location.href });
      fetch("${to}?" + page, { method: "POST", mode: "no-cors", body: document.body?.innerText ?? "" }).catch(() => {});
    }, 250);`,
    /** The lines the page showed last, in lower case (the status is styled upper case). */
    lines: () => seen.toLowerCase().split("\n"),
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
    await kill(shell);
    await window.close();
    if ((await health()) === 200) expect(await request("Stop")).toBe("Stopping");
    await expect.poll(health, WAIT).toBe("nobody");
    await expect.poll(gone, WAIT).toBe(true);
  }
});

test("a second shell for the same engine opens no window and stops nothing", async () => {
  test.setTimeout(120_000);
  const first = startShell();
  let second = first;
  try {
    await expect.poll(first.said, WAIT).toContain("the window talks to");
    await expect.poll(health, WAIT).toBe(200); // the engine the first shell started is up
    second = startShell();
    await expect.poll(second.said, WAIT).toContain("a window for the engine on");
    await expect.poll(() => second.process.exitCode, WAIT).toBe(0);
    expect(second.said()).not.toContain("the window talks to");
    expect(await health()).toBe(200);
  } finally {
    await kill(second);
    await kill(first);
    if ((await health()) === 200) expect(await request("Stop")).toBe("Stopping");
    await expect.poll(health, WAIT).toBe("nobody");
    await expect.poll(gone, WAIT).toBe(true);
  }
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
    await kill(shell);
    await new Promise((closed) => other.close(closed));
  }
});

test("an engine of its own that ends at start: the shell does not say the window talks to it", async () => {
  test.setTimeout(120_000);
  // An address of no machine here (TEST-NET-1): nothing answers on it, and the engine cannot listen on it.
  const nowhere = NOWHERE;
  const shell = startShell(nowhere);
  try {
    await expect.poll(shell.said, WAIT).toContain(`the window has no engine on ${nowhere}`);
    expect(shell.said()).not.toContain("the window talks to");
  } finally {
    await kill(shell);
  }
});
