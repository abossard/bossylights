// Spike #24, throwaway. 200 widgets, a rig preview and a waveform, fed by the spike engine
// at 44 Hz. Widgets show the value the engine reports; a press or a move sends a command.
// With ?run=auto the page drives itself through timed phases, measures, and sends the
// results to the engine. ?label= names the environment, ?engine= picks the engine.
export {};

type Frame = {
  frame: number;
  acks: [number, number][];
  widgets: [number, number][];
  fixtures: [number, number, number, number][];
  wave: number[];
};
type Kind = "button" | "slider" | "xy";
type Widget = {
  id: number;
  kind: Kind;
  x: number;
  y: number;
  w: number;
  h: number;
  el: HTMLDivElement;
  mark: HTMLDivElement | null;
  shown: [number, number];
};
type PhaseStats = { name: string; intervals: number[]; messages: number[]; started: number; ended: number };

const params = new URLSearchParams(location.search);
const ENGINE = params.get("engine") ?? "ws://127.0.0.1:7745/ws";
const LABEL = params.get("label") ?? "browser";
const AUTO = params.get("run") === "auto";
const COLS = 20;
const ROWS = 10;
const GAP = 4;
const WIDGETS = COLS * ROWS;
const FRAME_MS = 1000 / 44;

const surface = document.getElementById("surface") as HTMLDivElement;
const rig = document.getElementById("rig") as HTMLCanvasElement;
const waveCanvas = document.getElementById("wave") as HTMLCanvasElement;
const hud = document.getElementById("hud") as HTMLDivElement;

// ---------- connection ----------

let socket: WebSocket | undefined;
let status = "connecting";
let latest: Frame | undefined;
let latestAt = 0;
let lastMessageAt = 0;
let seq = 0;
const pendingPresses = new Map<number, number>(); // seq -> time of the press
const pressDelays: { ms: number; engineUs: number }[] = [];

function connect() {
  socket = new WebSocket(ENGINE);
  socket.onopen = () => {
    status = "connected";
    send({ log: `${LABEL} connected, ${innerWidth}x${innerHeight} at ${devicePixelRatio}x, ${location.href}` });
  };
  socket.onclose = () => {
    status = "disconnected";
    setTimeout(connect, 1000);
  };
  socket.onmessage = (message: MessageEvent<string>) => {
    const arrived = performance.now();
    const frame = JSON.parse(message.data) as Frame;
    for (const [s, engineUs] of frame.acks) {
      const pressed = pendingPresses.get(s);
      if (pressed === undefined) continue;
      pendingPresses.delete(s);
      if (measuring) pressDelays.push({ ms: arrived - pressed, engineUs });
    }
    if (measuring && lastMessageAt) measuring.messages.push(arrived - lastMessageAt);
    lastMessageAt = arrived;
    pushWave(frame.wave);
    latest = frame;
    latestAt = arrived;
  };
}

function send(command: object) {
  if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify(command));
}

function press(w: Widget, down: boolean, at: number) {
  seq += 1;
  if (down) pendingPresses.set(seq, at);
  send({ press: { id: w.id, down, seq } });
}

function set(w: Widget, x: number, y: number) {
  seq += 1;
  send({ set: { id: w.id, x, y, seq } });
}

// ---------- surface ----------

const widgets: Widget[] = [];
let topZ = 1;

function kindOf(i: number): Kind {
  const k = i % 10;
  if (k < 7) return "button";
  return k < 9 ? "slider" : "xy";
}

function build() {
  for (let id = 0; id < WIDGETS; id++) {
    const kind = kindOf(id);
    const el = document.createElement("div");
    el.className = `w ${kind}`;
    el.dataset.id = String(id);
    el.dataset.testid = `widget-${id}`;
    el.innerHTML = `<div class="grip"></div><div class="body"></div><div class="size"></div>`;
    const body = el.querySelector(".body") as HTMLDivElement;
    let mark: HTMLDivElement | null = null;
    if (kind !== "button") {
      mark = document.createElement("div");
      mark.className = kind === "slider" ? "fill" : "dot";
      body.append(mark);
    }
    surface.append(el);
    widgets.push({ id, kind, x: 0, y: 0, w: 0, h: 0, el, mark, shown: [-1, -1] });
  }
  layout();
}

function layout() {
  const r = surface.getBoundingClientRect();
  const cw = r.width / COLS;
  const ch = r.height / ROWS;
  for (const w of widgets) {
    w.x = (w.id % COLS) * cw + GAP / 2;
    w.y = Math.floor(w.id / COLS) * ch + GAP / 2;
    w.w = cw - GAP;
    w.h = ch - GAP;
    place(w);
  }
}

function place(w: Widget) {
  w.el.style.transform = `translate(${w.x}px, ${w.y}px)`;
  w.el.style.width = `${w.w}px`;
  w.el.style.height = `${w.h}px`;
}

/** Follows one pointer until it lifts. Mouse, pen and touch all arrive as pointer events. */
function track(down: PointerEvent, move: (e: PointerEvent, dx: number, dy: number) => void, up?: (e: PointerEvent) => void) {
  let lastX = down.clientX;
  let lastY = down.clientY;
  const onMove = (e: PointerEvent) => {
    if (e.pointerId !== down.pointerId) return;
    move(e, e.clientX - lastX, e.clientY - lastY);
    lastX = e.clientX;
    lastY = e.clientY;
  };
  const onUp = (e: PointerEvent) => {
    if (e.pointerId !== down.pointerId) return;
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    window.removeEventListener("pointercancel", onUp);
    up?.(e);
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
  window.addEventListener("pointercancel", onUp);
}

function valueAt(w: Widget, e: PointerEvent): [number, number] {
  const r = (w.el.querySelector(".body") as HTMLDivElement).getBoundingClientRect();
  const x = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
  const y = Math.min(1, Math.max(0, 1 - (e.clientY - r.top) / r.height));
  return w.kind === "slider" ? [y, 0] : [x, y];
}

surface.addEventListener("pointerdown", (e) => {
  const target = e.target as Element;
  const el = target.closest<HTMLDivElement>(".w");
  const part = target.closest(".grip, .size, .body");
  if (!el || !part) return;
  e.preventDefault();
  const w = widgets[Number(el.dataset.id)];
  w.el.style.zIndex = String(++topZ);
  if (part.classList.contains("grip")) {
    track(e, (_, dx, dy) => {
      w.x += dx;
      w.y += dy;
      place(w);
    });
  } else if (part.classList.contains("size")) {
    track(e, (_, dx, dy) => {
      w.w = Math.max(30, w.w + dx);
      w.h = Math.max(30, w.h + dy);
      place(w);
    });
  } else if (w.kind === "button") {
    press(w, true, e.timeStamp);
    track(e, () => {}, (up) => press(w, false, up.timeStamp));
  } else {
    set(w, ...valueAt(w, e));
    track(e, (move) => set(w, ...valueAt(w, move)));
  }
});

function showWidgets(frame: Frame) {
  for (const w of widgets) {
    const [a, b] = frame.widgets[w.id];
    if (a === w.shown[0] && b === w.shown[1]) continue;
    w.shown = [a, b];
    if (w.kind === "button") w.el.classList.toggle("on", a > 0.5);
    else if (w.kind === "slider" && w.mark) w.mark.style.height = `${a * 100}%`;
    else if (w.mark) {
      w.mark.style.left = `${a * 100}%`;
      w.mark.style.top = `${(1 - b) * 100}%`;
    }
  }
}

// ---------- rig preview: 100 fixtures on four trusses, a beam and a glow each ----------

function fit(canvas: HTMLCanvasElement) {
  const r = canvas.getBoundingClientRect();
  const dpr = devicePixelRatio;
  if (canvas.width !== Math.round(r.width * dpr)) canvas.width = Math.round(r.width * dpr);
  if (canvas.height !== Math.round(r.height * dpr)) canvas.height = Math.round(r.height * dpr);
  return canvas.getContext("2d") as CanvasRenderingContext2D;
}

function drawRig(frame: Frame | undefined) {
  const g = fit(rig);
  const { width: W, height: H } = rig;
  g.fillStyle = "#050505";
  g.fillRect(0, 0, W, H);
  if (!frame) return;
  const trusses = 4;
  const perTruss = frame.fixtures.length / trusses;
  for (let t = 0; t < trusses; t++) {
    const y = H * (0.12 + t * 0.12);
    g.strokeStyle = "#333";
    g.lineWidth = 3;
    g.beginPath();
    g.moveTo(W * 0.03, y);
    g.lineTo(W * 0.97, y);
    g.stroke();
    for (let i = 0; i < perTruss; i++) {
      const [r, gr, b, level] = frame.fixtures[t * perTruss + i];
      const x = W * (0.05 + (0.9 * i) / (perTruss - 1));
      const a = level / 255;
      const color = `rgba(${r},${gr},${b},`;
      const beam = g.createLinearGradient(x, y, x, H);
      beam.addColorStop(0, `${color}${0.45 * a})`);
      beam.addColorStop(1, `${color}0)`);
      g.fillStyle = beam;
      g.beginPath();
      g.moveTo(x - 3, y);
      g.lineTo(x + 3, y);
      g.lineTo(x + 22, H);
      g.lineTo(x - 22, H);
      g.closePath();
      g.fill();
      const glow = g.createRadialGradient(x, y, 0, x, y, 14);
      glow.addColorStop(0, `${color}${a})`);
      glow.addColorStop(1, `${color}0)`);
      g.fillStyle = glow;
      g.beginPath();
      g.arc(x, y, 14, 0, Math.PI * 2);
      g.fill();
    }
  }
}

// ---------- waveform: the last 6 seconds, scrolling ----------

const WAVE_SECONDS = 6;
const BINS_PER_FRAME = 32;
const ring = new Float32Array(44 * BINS_PER_FRAME * WAVE_SECONDS);
let ringAt = 0;

function pushWave(bins: number[]) {
  for (const v of bins) {
    ring[ringAt] = v;
    ringAt = (ringAt + 1) % ring.length;
  }
}

function drawWave(now: number) {
  const g = fit(waveCanvas);
  const { width: W, height: H } = waveCanvas;
  g.fillStyle = "#050505";
  g.fillRect(0, 0, W, H);
  // between engine frames, slide on so the scroll stays smooth
  const slide = Math.min(1, (now - latestAt) / FRAME_MS) * BINS_PER_FRAME;
  const perColumn = ring.length / W;
  const mid = H / 2;
  g.strokeStyle = "#5ad1e0";
  g.lineWidth = 1;
  g.beginPath();
  for (let x = 0; x < W; x++) {
    const start = ringAt + x * perColumn + slide;
    let peak = 0;
    for (let k = 0; k < perColumn; k++) peak = Math.max(peak, ring[Math.floor(start + k) % ring.length]);
    g.moveTo(x + 0.5, mid - peak * mid * 0.95);
    g.lineTo(x + 0.5, mid + peak * mid * 0.95);
  }
  g.stroke();
}

// ---------- the loop and the counters ----------

let measuring: PhaseStats | undefined;
let lastPaint = 0;
let shownFrame: Frame | undefined;
const recent: number[] = [];

function paint(now: number) {
  if (lastPaint && measuring) measuring.intervals.push(now - lastPaint);
  recent.push(now);
  while (recent.length && recent[0] < now - 1000) recent.shift();
  lastPaint = now;
  if (latest && latest !== shownFrame) {
    showWidgets(latest);
    shownFrame = latest;
  }
  drawRig(latest);
  drawWave(now);
  requestAnimationFrame(paint);
}

setInterval(() => {
  hud.textContent = `${LABEL}  ${status}  frame ${latest?.frame ?? "-"}\n${recent.length} fps  ${measuring ? `measuring ${measuring.name}` : phaseNote}`;
}, 250);

// ---------- automatic run ----------

let phaseNote = AUTO ? "auto run starting" : "interactive";
const phases: ReturnType<typeof summarise>[] = [];
const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));
const nextPaint = () => new Promise<number>((done) => requestAnimationFrame(done));

function percentile(values: number[], p: number) {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)];
}
const round = (v: number) => Math.round(v * 10) / 10;

function summarise(s: PhaseStats) {
  const seconds = (s.ended - s.started) / 1000;
  return {
    name: s.name,
    seconds: round(seconds),
    fps: round(s.intervals.length / seconds),
    frameMs: { p50: round(percentile(s.intervals, 50)), p95: round(percentile(s.intervals, 95)), max: round(Math.max(0, ...s.intervals)) },
    framesOver25ms: s.intervals.filter((v) => v > 25).length,
    engineFramesPerSecond: round(s.messages.length / seconds),
    engineGapMs: { p95: round(percentile(s.messages, 95)), max: round(Math.max(0, ...s.messages)) },
  };
}

async function phase(name: string, ms: number, drive?: (until: number) => Promise<void>) {
  measuring = { name, intervals: [], messages: [], started: performance.now(), ended: 0 };
  const until = performance.now() + ms;
  await Promise.all([sleep(ms), drive?.(until)]);
  measuring.ended = performance.now();
  phases.push(summarise(measuring));
  send({ log: `${LABEL} ${name}: ${JSON.stringify(phases[phases.length - 1])}` });
  measuring = undefined;
}

function pointer(type: string, x: number, y: number, target: EventTarget, id: number) {
  const e = new PointerEvent(type, { pointerId: id, pointerType: "mouse", isPrimary: true, bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons: type === "pointerup" ? 0 : 1 });
  target.dispatchEvent(e);
}

function centre(el: Element) {
  const r = el.getBoundingClientRect();
  return [r.left + r.width / 2, r.top + r.height / 2] as const;
}

/** Drags widget 0 by its grip in a circle, one move per painted frame. */
async function dragInCircle(until: number) {
  const grip = widgets[0].el.querySelector(".grip") as HTMLDivElement;
  const [x0, y0] = centre(grip);
  pointer("pointerdown", x0, y0, grip, 901);
  const start = performance.now();
  while (performance.now() < until) {
    const t = (performance.now() - start) / 1000;
    pointer("pointermove", x0 + 200 - 200 * Math.cos(t * 2), y0 + 150 * Math.sin(t * 2), window, 901);
    await Promise.race([nextPaint(), sleep(100)]); // a hidden page paints nothing: never wait forever
  }
  pointer("pointerup", x0, y0, window, 901);
}

/** Presses a random button every 150 ms and lets go 60 ms later. */
async function pressButtons(until: number) {
  const buttons = widgets.filter((w) => w.kind === "button" && w.id !== 0);
  let n = 0;
  while (performance.now() < until - 200) {
    const w = buttons[(n++ * 37) % buttons.length];
    const body = w.el.querySelector(".body") as HTMLDivElement;
    const [x, y] = centre(body);
    pointer("pointerdown", x, y, body, 902);
    await sleep(60);
    pointer("pointerup", x, y, window, 902);
    await sleep(90);
  }
}

async function autoRun() {
  while (!latest) await sleep(100);
  phaseNote = "warming up";
  await sleep(3000);
  await phase("idle", 15000);
  await phase("drag", 15000, dragInCircle);
  send({ animate: { on: true } });
  await phase("update", 15000);
  send({ animate: { on: false } });
  await phase("press", 20000, pressButtons);
  const ms = pressDelays.map((d) => d.ms);
  const engineMs = pressDelays.map((d) => d.engineUs / 1000);
  const results = {
    label: LABEL,
    userAgent: navigator.userAgent,
    viewport: { width: innerWidth, height: innerHeight, devicePixelRatio },
    phases,
    pressToFrame: {
      presses: ms.length,
      lost: pendingPresses.size,
      uiMs: { p50: round(percentile(ms, 50)), p95: round(percentile(ms, 95)), max: round(Math.max(0, ...ms)) },
      engineWaitMs: { p50: round(percentile(engineMs, 50)), p95: round(percentile(engineMs, 95)), max: round(Math.max(0, ...engineMs)) },
    },
  };
  send({ results });
  (window as unknown as { spikeResults: unknown }).spikeResults = results;
  phaseNote = "done";
}

build();
addEventListener("resize", layout);
connect();
requestAnimationFrame(paint);
if (AUTO) void autoRun();
