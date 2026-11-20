// Where a bar sits and how wide it is, after a drag or a key: one calculation for both. Data and calculations.
// The readout under the bar is the place: it is read, moved and written back, and it is what is sent.

/** A move in steps: along the line, across rows, and wider or narrower. */
export interface Move {
  across: number;
  down: number;
  wider: number;
}

/** A drag moves one step per this many px. */
export const STEP_PX = 20;

/** How far a drag went, in steps. */
export const dragged = (dx: number, dy: number): Move => ({ across: Math.round(dx / STEP_PX), down: Math.round(dy / STEP_PX), wider: 0 });

interface Place {
  at: number;
  width: number;
}

interface Placement {
  /** Shift and an arrow change its width. */
  resizes: boolean;
  read: (readout: string) => Place | undefined;
  write: (place: Place) => string;
  move: (place: Place, by: Move) => Place;
}

const clamp = (value: number, low: number, high: number) => Math.max(low, Math.min(high, value));

/** A count of quarter beats as it reads: 5 is "1¼". */
const quarters = (count: number) => {
  const whole = Math.floor(count / 4);
  const part = ["", "¼", "½", "¾"][count % 4] ?? "";
  return whole === 0 ? part : `${whole}${part}`;
};
const QUARTERS = /^(\d*)([¼½¾]?)$/;
const readQuarters = (text: string) => {
  const m = QUARTERS.exec(text);
  if (!m || (m[1] === "" && m[2] === "")) return undefined;
  return Number(m[1] || 0) * 4 + ["", "¼", "½", "¾"].indexOf(m[2] ?? "");
};

const UNIVERSE = 512;
const ROW = 32;

/** The controls that are placed, by their row. */
const PLACEMENTS: Record<string, Placement> = {
  // "17 to 23": the first and last address. Left and right move by one address, up and down by a row of 32.
  "patch.grid.light": {
    resizes: false,
    read: (readout) => {
      const m = /^(\d+) to (\d+)$/.exec(readout);
      return m ? { at: Number(m[1]), width: Number(m[2]) - Number(m[1]) + 1 } : undefined;
    },
    write: ({ at, width }) => `${at} to ${at + width - 1}`,
    move: ({ at, width }, by) => ({ at: clamp(at + by.across + by.down * ROW, 1, UNIVERSE - width + 1), width }),
  },
  // "At beat 1¼, 1 beat": where it starts and how long it lasts, in quarter beats. Never narrower than a quarter.
  "looks.seq.bar": {
    resizes: true,
    read: (readout) => {
      const m = /^At beat (\S+), (\S+) beats?$/.exec(readout);
      const at = m?.[1] === undefined ? undefined : readQuarters(m[1]);
      const width = m?.[2] === undefined ? undefined : readQuarters(m[2]);
      return at === undefined || width === undefined ? undefined : { at: at - 4, width };
    },
    write: ({ at, width }) => `At beat ${quarters(at + 4)}, ${quarters(width)} ${width > 4 ? "beats" : "beat"}`,
    move: ({ at, width }, by) => ({ at: Math.max(0, at + by.across), width: Math.max(1, width + by.wider) }),
  },
};

/** The control is placed: a drag or a key moves its readout. */
export const placed = (id: string): boolean => PLACEMENTS[id] !== undefined;

/** Shift and an arrow change its width. */
export const resizes = (id: string): boolean => PLACEMENTS[id]?.resizes ?? false;

/** The readout after a move. Undefined: the control is not placed, or its readout is not a place. */
export function place(id: string, readout: string, by: Move): string | undefined {
  const placement = PLACEMENTS[id];
  const now = placement?.read(readout);
  return placement && now ? placement.write(placement.move(now, by)) : undefined;
}
