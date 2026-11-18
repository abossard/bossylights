// One control, live: the primitive of its row, drawn from its recipe and a `Look`, its gestures reported as acts.
// The only place that turns a row into a primitive with props.
import type { JSX } from "solid-js";
import type { Control } from "../controls/spec.ts";
import { Block, Button, Fader, Field, Handle, Item, named, offWith, Pad, Picker, XY, type PickOption, type StatusProps } from "../kit/index.ts";
import type { Act } from "./act.ts";
import { linesOf, recipe } from "./compose.ts";
import type { Look } from "./look.ts";
import { gobos } from "./drawings.tsx";
import { USES } from "./uses.ts";

/** A press that comes up within this is short. Longer, it was a hold (the pad machine). */
const SHORT_PRESS_MS = 350;

const oneOf = <const T extends string>(value: string | undefined, all: readonly T[], otherwise: T): T => all.find((v) => v === value) ?? otherwise;
const oneOrNone = <const T extends string>(value: string | undefined, all: readonly T[]): T | undefined => all.find((v) => v === value);
/** `{ key: value }` when there is a value: an optional prop is left out, never undefined. */
const some = <K extends string, V>(key: K, value: V | undefined | ""): { [P in K]?: V } => (value === undefined || value === "" ? {} : ({ [key]: value } as { [P in K]?: V }));

/** The lamp of the menu bar item, by the name of its look. */
const LAMPS: Record<string, StatusProps> = {
  mute: { kind: "lamp", label: "Engine runs", colour: "mute" },
  paper: { kind: "lamp", label: "LIVE is on", colour: "paper" },
  danger: { kind: "lamp", label: "Trouble", colour: "danger" },
  empty: { kind: "lamp", label: "Not connected", colour: "mute", fill: "outline" },
};

export function Live(props: { control: Control; look: Look; act: (act: Act) => void }): JSX.Element {
  const c = props.control;
  const r = recipe(c);
  const holds = linesOf(c);
  const l = () => props.look;
  const press = () => props.act({ is: "press" });
  const pick = (option: string) => props.act({ is: "pick", option });
  /** The whole control is off, with its reason. */
  const off = () => offWith(l().off && !l().offOption ? l().off : undefined);
  const sideOff = (option: string | undefined) => (l().off && option !== undefined && l().offOption === option ? l().off : undefined);
  const drags = USES[c.id]?.machine === "drag";
  const lift = () => props.act({ is: "lift" });
  const drop = () => props.act({ is: "drop" });
  const cancel = () => props.act({ is: "cancel" });
  const dragged = () => (drags ? { onLift: lift, onDrop: drop, onCancel: cancel } : {});
  /** A step from the keyboard: lifted and dropped at once. */
  const nudge = () => {
    lift();
    drop();
  };
  const standing = () => (l().on ? "picked" : oneOf(l().variant, ["lifted", "dashed", "clash"], "plain"));
  /** What every button of a row takes, whatever its kind. */
  const button = () => ({
    label: l().label,
    colour: l().colour,
    holds,
    onPress: press,
    ...(r.props.held ? { held: { onDown: () => props.act({ is: "down" }), onUp: (heldMs: number) => props.act({ is: "up", long: heldMs > SHORT_PRESS_MS }) } } : {}),
    ...off(),
    ...some("sub", l().sub),
    ...some("fires", oneOrNone(r.props.fires, ["down"])),
    ...some("size", oneOrNone(r.props.size, ["small", "large"])),
  });
  const options = (): PickOption[] => named(l().options);

  switch (c.kit) {
    case "Button.action":
      return <Button {...button()} filled={l().on} {...some("glyph", r.props.glyph)} {...some("own", r.props.own)} />;
    case "Button.toggle":
      return (
        <Button
          {...button()}
          kind="toggle"
          state={l().on ? "on" : l().variant === "ready" ? "ready" : "off"}
          quiet={r.props.quiet === "yes"}
          over={l().variant === "over"}
          {...some("mark", oneOrNone(r.props.mark, ["lamp", "chevron", "knob"]))}
          {...some("lamp", oneOrNone(r.props.lamp, ["ok", "warn", "danger"]))}
          {...(r.props.mark === "chevron" ? { sub: l().on && l().value === 0 ? "Empty. Drag a look here" : String(l().value) } : {})}
        />
      );
    case "Pad":
      return (
        <Pad
          label={l().label}
          colours={["#ff9d2e", "#ffd27a"]}
          state={oneOf(l().variant, ["idle", "manual", "flash", "kept", "show", "autopilot"], "idle")}
          {...off()}
          onDown={() => props.act({ is: "down" })}
          onUp={(heldMs) => props.act({ is: "up", long: heldMs > SHORT_PRESS_MS })}
        />
      );
    case "Fader":
      return <Fader label={l().label} value={l().value} {...off()} onChange={(value) => props.act({ is: "set", value, value2: l().value2 })} />;
    case "XY.position":
    case "XY.colour":
      return <XY kind={c.kit === "XY.colour" ? "colour" : "position"} label={l().label} across={l().value} up={l().value2} held={l().on} {...off()} onChange={(value, value2) => props.act({ is: "set", value, value2 })} />;
    case "Picker.tabs":
      return <Picker kind="tabs" label={c.name} options={options().map((o, i) => (l().variant === "marked" && i === 1 ? { ...o, marked: true } : o))} picked={l().picked} onPick={pick} />;
    case "Picker.segments":
      return <Picker kind="segments" label={c.name} options={options()} picked={l().picked} {...some("size", oneOrNone(r.props.size, ["small"]))} {...some("fires", oneOrNone(r.props.fires, ["down"]))} {...off()} onPick={pick} />;
    case "Picker.pictures":
      return <Picker kind="pictures" label={c.name} options={gobos(l().options)} picked={l().picked} {...off()} onPick={pick} />;
    case "Picker.cards":
      return <Picker kind="cards" label={c.name} options={options().map((o) => ({ ...o, ...offWith(sideOff(o.name)) }))} picked={l().picked} onPick={pick} />;
    case "Picker.list":
      return <Picker kind="list" label={l().label} options={options()} picked={l().picked} {...off()} onPick={pick} />;
    case "Field.text":
    case "Field.search":
      return <Field kind={c.kit === "Field.search" ? "search" : "text"} label={l().label} value={l().picked} {...off()} onText={(text) => props.act({ is: "text", text })} />;
    case "Field.steps":
      return (
        <Field
          kind="steps"
          label={l().label}
          value={l().sub}
          less={l().options[0] ?? "Less"}
          more={l().options[1] ?? "More"}
          {...off()}
          {...some("lessOff", sideOff(l().options[0]))}
          {...some("moreOff", sideOff(l().options[1]))}
          onStep={(by) => props.act({ is: "step", by })}
        />
      );
    case "Item.row":
      return (
        <Item
          kind="row"
          title={l().label}
          {...some("sub", l().sub)}
          // A state in a word at its right: "Playing", "Clash". Red is an outline, so the row stays readable.
          marks={/^[A-Z]/.test(l().variant) ? [{ kind: "pill", label: l().variant, colour: l().colour, fill: l().colour === "danger" ? "outline" : "solid" }] : []}
          standing={standing()}
          {...off()}
          onPress={press}
          {...dragged()}
        />
      );
    case "Item.tile":
      return <Item kind="tile" title={l().label} {...some("sub", l().sub)} standing={standing()} onPress={press} />;
    case "Item.cell":
      return (
        <Item
          kind="cell"
          title={l().label}
          sub="17"
          marks={[{ kind: "value", label: String(l().value) }]}
          colour={l().variant === "held" ? "manual" : "plain"}
          standing={l().on ? "picked" : l().variant === "changing" ? "dashed" : standing()}
          says={`Channel 17, ${l().label || "not used"}, at ${l().value}`}
          onPress={press}
        />
      );
    case "Item.entry":
      return <Item kind="entry" title={l().label} {...some("sub", l().sub)} {...some("lead", LAMPS[l().variant])} standing={standing()} {...off()} onPress={press} />;
    case "Item.chip":
      return <Item kind="chip" title={l().label} onRemove={() => props.act({ is: "remove" })} />;
    case "Item.strip": {
      const lane = (name: string, colour: "manual" | "show" | "autopilot", runs: string): StatusProps => ({ kind: "pill", name, label: runs || "nothing", colour, fill: runs ? "solid" : "dashed" });
      const lanes = () => [lane("Manual", "manual", l().on ? "Warm wash" : ""), lane("Show", "show", l().on ? "Chase L-R" : ""), lane("Autopilot", "autopilot", "")];
      return (
        <Item
          kind="strip"
          title="Now"
          lead={{ kind: "flag", label: "NOW" }}
          marks={lanes()}
          says={`Now strip. ${lanes().map((s) => `${s.name}: ${s.label}`).join(". ")}. Opens the Console`}
          {...off()}
          onPress={press}
        />
      );
    }
    case "Block.bar":
      return <Block kind="bar" label={l().label} {...some("sub", l().sub)} colour={l().colour} standing={standing()} onPress={press} {...dragged()} />;
    case "Block.frame":
      return <Block kind="frame" label={l().label} {...some("sub", l().sub)} standing={standing()} onLift={lift} onDrop={drop} onCancel={cancel} onNudge={nudge} onResize={nudge} onRemove={() => props.act({ is: "remove" })} />;
    case "Handle.edge":
    case "Handle.corner":
      return <Handle kind={c.kit === "Handle.corner" ? "corner" : "edge"} label={l().label} onLift={lift} onDrop={drop} onCancel={cancel} onNudge={nudge} />;
  }
}
