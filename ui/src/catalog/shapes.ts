// What a text field takes, by its row: a text that does not fit is refused with these words. Calculations.
// A field with no shape takes any text. An empty field keeps the old value: the kit does that for every field.

const whole = (text: string, low: number, high: number) => /^\d+$/.test(text) && Number(text) >= low && Number(text) <= high;
const unless = (fits: boolean, reason: string) => (fits ? undefined : reason);

const range = (text: string) => {
  const m = /^(\d+)(?: to (\d+))?$/.exec(text);
  const first = Number(m?.[1] ?? 0);
  const last = Number(m?.[2] ?? first);
  return first >= 1 && last >= first;
};

/** Why a text does not fit the field of a control. Undefined: it fits. */
export const SHAPES: Record<string, (text: string) => string | undefined> = {
  "inout.osc.port": (text) => unless(whole(text, 1, 65535), "A port is a number from 1 to 65535"),
  "inout.out.universes": (text) => unless(range(text), "One universe, as 1, or a range, as 2 to 9"),
  "inout.mapping.message": (text) => unless(/^(Note|CC) \d+$/.test(text) && whole(text.split(" ")[1] ?? "", 0, 127), "A note, as Note 36, or a controller, as CC 48"),
  "patch.panel.start": (text) => unless(whole(text, 1, 32768), "A universe is a number, 1 or more"),
};
