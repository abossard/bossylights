// How every control is put together from the kit: the primitive of its row, its kind, its colour and
// the props that never change. Data and calculations. `Live.tsx` draws from it, `docs/design/kit.md` is written from it.
import { kindOf, kitNames, primitiveOf, type Control, type Kit, type Primitive } from "../controls/spec.ts";
import type { Colour } from "../kit/common.ts";
import { inState, usual } from "./look.ts";
import { isHeld } from "./uses.ts";

/** What never changes about how a control is drawn. What changes with its state comes from its `Look`. */
export interface Recipe {
  primitive: Primitive;
  /** Empty for a primitive of one kind. */
  kind: string;
  colour: Colour;
  /** The other props that are fixed, as they are written on the primitive. */
  props: Readonly<Record<string, string>>;
  /** The primitives it carries inside, by `Primitive.kind`. */
  inside: readonly string[];
}

/** The props a control fixes that neither its row nor its kind says. */
const FIXED: Record<string, Readonly<Record<string, string>>> = {
  "top.live": { mark: "knob", size: "large" },
  // The owner's review: the DJ link lamp is green when linked, Autopilot fills marigold when on.
  "top.indicator.dj": { mark: "lamp", quiet: "yes", lamp: "ok" },
  "top.indicator.recording": { mark: "lamp", quiet: "yes" },
  "top.indicator.autopilot": { mark: "lamp" },
  "top.indicator.lock": { mark: "lamp", quiet: "yes" },
  "console.running.fold": { mark: "chevron", quiet: "yes" },
  "looks.library.group.fold": { mark: "chevron", quiet: "yes" },
  "patch.universes.panel": { mark: "chevron", quiet: "yes" },
  "looks.step.colour.remove": { glyph: "×", own: "#ff3b30" },
  "looks.step.colour.add": { glyph: "+", own: "#ffb300" },
};

/** What a primitive of a kind always carries inside. */
const INSIDE: Partial<Record<Kit, readonly string[]>> = {
  Pad: ["Status.flag", "Status.pill"],
  Fader: ["Status.value"],
  "Field.steps": ["Button.action", "Status.value", "Button.action"],
  "Item.row": ["Status.pill"],
  "Item.cell": ["Status.value"],
  "Item.chip": ["Button.action"],
  "Item.strip": ["Status.flag", "Status.pill"],
};

/** Every line a control shows in any of its states, when it shows more than one: it keeps the width of the widest. */
export function linesOf(control: Control): string[] {
  const looks = [usual(control), ...control.states.map((state) => inState(control, state))];
  const lines = new Set(looks.flatMap((look) => [look.label, look.sub]).filter(Boolean));
  return lines.size > 1 ? [...lines] : [];
}

export function recipe(control: Control): Recipe {
  const primitive = primitiveOf(control.kit);
  const props: Record<string, string> = { ...FIXED[control.id] };
  if ((primitive === "Button" || primitive === "Picker") && control.gestures.press.includes("on the way down")) props.fires = "down";
  if (primitive === "Button" && isHeld(control.id)) props.held = "short press and hold";
  if (control.kit === "Picker.segments" && control.gestures.touch.includes("30 px")) props.size = "small";
  if (primitive === "Button" && linesOf(control).length) props.holds = `${linesOf(control).length} lines`;
  const lamp = control.id === "menubar.item" ? ["Status.lamp"] : [];
  return { primitive, kind: kindOf(control.kit), colour: usual(control).colour, props, inside: [...(INSIDE[control.kit] ?? []), ...lamp] };
}

/** The controls that are drawn the same way but for their words: one group per primitive, kind and fixed props. */
export interface Group {
  primitive: Primitive;
  kind: string;
  controls: Control[];
}

/** Every control under its primitive and kind, primitives and kinds in the order of the kit, controls in the order of the table. */
export function byFunction(controls: readonly Control[], primitives: readonly Primitive[]): Group[] {
  const groups = new Map<string, Group>();
  for (const c of controls) {
    const group = groups.get(c.kit) ?? { primitive: primitiveOf(c.kit), kind: kindOf(c.kit), controls: [] };
    group.controls.push(c);
    groups.set(c.kit, group);
  }
  return primitives.flatMap(kitNames).flatMap((name) => groups.get(name) ?? []);
}
