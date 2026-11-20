// What a text field takes, by its row: a text that does not fit is refused with these words. Calculations.
// A field with no shape takes any text. An empty field keeps the old value: the kit does that for every field.

/** The highest universe a show can use. */
const UNIVERSES = 32768;

const whole = (text: string, low: number, high: number) => /^\d+$/.test(text) && Number(text) >= low && Number(text) <= high;
const unless = (fits: boolean, reason: string) => (fits ? undefined : reason);

const range = (text: string) => {
  const m = /^(\d+)(?: to (\d+))?$/.exec(text);
  const first = Number(m?.[1] ?? 0);
  const last = Number(m?.[2] ?? first);
  return first >= 1 && last >= first && last <= UNIVERSES;
};

const size = (text: string) => {
  const m = /^(\d+) x (\d+)$/.exec(text);
  return whole(m?.[1] ?? "", 1, 1024) && whole(m?.[2] ?? "", 1, 1024);
};

/** Why a text does not fit the field of a control. Undefined: it fits. */
export const SHAPES: Record<string, (text: string) => string | undefined> = {
  "inout.osc.port": (text) => unless(whole(text, 1, 65535), "A port is a number from 1 to 65535"),
  "inout.out.universes": (text) => unless(range(text), `One universe from 1 to ${UNIVERSES}, as 1, or a range, as 2 to 9`),
  "inout.mapping.message": (text) => unless(/^(Note|CC) \d+$/.test(text) && whole(text.split(" ")[1] ?? "", 0, 127), "A note, as Note 36, or a controller, as CC 48"),
  "patch.panel.start": (text) => unless(whole(text, 1, UNIVERSES), `A universe is a number from 1 to ${UNIVERSES}`),
  "patch.panel.size": (text) => unless(size(text), "Columns x rows, as 16 x 16, each from 1 to 1024"),
};

/** A text that fits, as it is sent: a number without its leading zeros, 007 as 7. */
export const tidy = (text: string): string => text.replace(/\d+/g, (n) => String(Number(n)));
