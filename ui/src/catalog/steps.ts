// What a stepper reads after one step: the value between its two buttons, and the button that is off at a limit.
// Data and calculations. The readout is the value: it is read, stepped and written back, and it is what is sent.

interface Stepper {
  read: (readout: string) => number | undefined;
  write: (value: number) => string;
  /** One step down (-1) or up (1). */
  step: (value: number, by: -1 | 1) => number;
  low: number;
  high: number;
  /** Why the button towards each limit is off there. */
  atLow: string;
  atHigh: string;
}

/** A count of quarter beats as it reads: 5 is "1¼ beats", 1 is "¼ beat". */
const QUARTER = ["", "¼", "½", "¾"];
const quarters = (count: number) => {
  const whole = Math.floor(count / 4);
  const number = `${whole === 0 ? "" : whole}${QUARTER[count % 4] ?? ""}`;
  return `${number} ${count > 4 ? "beats" : "beat"}`;
};
const readQuarters = (readout: string) => {
  const m = /^(\d*)([¼½¾]?) beats?$/.exec(readout);
  if (!m || (m[1] === "" && m[2] === "")) return undefined;
  return Number(m[1] || 0) * 4 + QUARTER.indexOf(m[2] ?? "");
};

const beats = (count: number) => `${count} ${count === 1 ? "beat" : "beats"}`;
const readBeats = (readout: string) => {
  const m = /^(\d+) beats?$/.exec(readout);
  return m ? Number(m[1]) : undefined;
};

const readWhole = (readout: string) => (/^\d+$/.test(readout) ? Number(readout) : undefined);

/** A universe has 512 addresses. The sample light has 7 channels: the last address it fits at is 506. */
const address = (high: number): Stepper => ({
  read: readWhole,
  write: String,
  step: (value, by) => value + by,
  low: 1,
  high,
  atLow: "First address",
  atHigh: "Last address that fits",
});

const STEPPERS: Record<string, Stepper> = {
  // A quarter beat narrower or wider. The row names no widest.
  "looks.seq.width": { read: readQuarters, write: quarters, step: (value, by) => value + by, low: 1, high: Number.POSITIVE_INFINITY, atLow: "A quarter beat is the least", atHigh: "" },
  // Halves or doubles, from 1 to 32 beats.
  "looks.pattern.measure": { read: readBeats, write: beats, step: (value, by) => (by > 0 ? value * 2 : value / 2), low: 1, high: 32, atLow: "1 beat is the least", atHigh: "32 beats is the most" },
  "patch.details.address": address(512 - 7 + 1),
  "patch.add.address": address(512 - 7 + 1),
};

/** The readout after one step, and the button that is off at the limit it reached. Undefined: not a stepper, or a readout that is not its value. */
export function stepped(id: string, readout: string, by: -1 | 1, buttons: readonly string[]): { sub: string; off: string; offOption: string } | undefined {
  const stepper = STEPPERS[id];
  const now = stepper?.read(readout);
  if (!stepper || now === undefined) return undefined;
  const next = Math.max(stepper.low, Math.min(stepper.high, stepper.step(now, by)));
  return { sub: stepper.write(next), ...limit(stepper, next, buttons) };
}

/** The button that is off at a value, with why. */
function limit(stepper: Stepper, value: number, buttons: readonly string[]): { off: string; offOption: string } {
  if (value <= stepper.low) return { off: stepper.atLow, offOption: buttons[0] ?? "" };
  if (value >= stepper.high) return { off: stepper.atHigh, offOption: buttons[1] ?? "" };
  return { off: "", offOption: "" };
}

/** The look of a stepper at one of its limits: the value there, the button that is off, and why. Undefined: not a stepper. */
export function atLimit(id: string, side: -1 | 1, buttons: readonly string[]): { sub: string; off: string; offOption: string } | undefined {
  const stepper = STEPPERS[id];
  if (!stepper || !Number.isFinite(side < 0 ? stepper.low : stepper.high)) return undefined;
  const value = side < 0 ? stepper.low : stepper.high;
  return { sub: stepper.write(value), ...limit(stepper, value, buttons) };
}
