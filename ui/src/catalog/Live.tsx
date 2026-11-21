// One control, live: the primitive of its row, drawn from its recipe and a `Look`, its gestures reported as acts.
// The only place that turns a row into a primitive with props.
import { createMemo, Show, type JSX } from "solid-js";
import type { Control } from "../controls/spec.ts";
import { Block, Button, Fader, Field, Handle, Item, named, offWith, Pad, Picker, Status, XY, type PickOption, type StatusProps } from "../kit/index.ts";
import type { Act } from "./act.ts";
import { linesOf, recipe } from "./compose.ts";
import type { Look } from "./look.ts";
import { gobos, PICTURES } from "./drawings.tsx";
import { dragged as stepsOf, placed, resizes } from "./place.ts";
import { NOW } from "./samples.ts";
import { SHAPES, tidy } from "./shapes.ts";
import { USES } from "./uses.ts";

/** A press that comes up within this is short. Longer, it was a hold (the pad machine). */
const SHORT_PRESS_MS = 350;

const oneOf = <const T extends string>(value: string | undefined, all: readonly T[], otherwise: T): T => all.find((v) => v === value) ?? otherwise;
const oneOrNone = <const T extends string>(value: string | undefined, all: readonly T[]): T | undefined => all.find((v) => v === value);
/** `{ key: value }` when there is a value: an optional prop is left out, never undefined. */
const some = <K extends string, V>(key: K, value: V | undefined | ""): { [P in K]?: V } => (value === undefined || value === "" ? {} : ({ [key]: value } as { [P in K]?: V }));

/** Two lists of options say the same: a picker keeps the old objects, so the option with the focus stays. */
const sameOptions = (a: readonly PickOption[], b: readonly PickOption[]) =>
  a.length === b.length && a.every((o, i) => o.name === b[i]?.name && o.off === b[i]?.off && o.marked === b[i]?.marked && o.sub === b[i]?.sub);

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
  /** Dropped. `dx`, `dy`: how far it went, in px. A keyboard step has none: it always lands elsewhere. */
  const drop = (dx?: number, dy?: number) => props.act(dx === undefined ? { is: "drop" } : { is: "drop", by: stepsOf(dx, dy ?? 0) });
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
  const options = createMemo((): PickOption[] => (PICTURES[c.id] ?? named)(l().options), [], { equals: sameOptions });
  const tabs = createMemo(() => options().map((o, i) => (l().variant === "marked" && i === 1 ? { ...o, marked: true } : o)), [], { equals: sameOptions });
  const cards = createMemo(() => options().map((o) => ({ ...o, ...offWith(sideOff(o.name)) })), [], { equals: sameOptions });
  const pictures = createMemo(() => (PICTURES[c.id] ?? gobos)(l().options), [], { equals: sameOptions });
  /** A bar that is placed: a drag and the arrows move it, its readout says where. */
  const placing = () =>
    placed(c.id)
      ? {
          onDrop: (dx: number, dy: number) => props.act({ is: "drop", by: stepsOf(dx, dy) }),
          onNudge: (across: number, down: number) => props.act({ is: "nudge", by: { across, down, wider: 0 } }),
          ...(resizes(c.id) ? { onResize: (wider: number) => props.act({ is: "nudge", by: { across: 0, down: 0, wider } }) } : {}),
        }
      : {};
  /** Leaving the field saves where the row says so. */
  const saves = c.gestures.keyboard.includes("leaving the field saves") ? "enter or leave" : "enter";

  switch (c.kit) {
    case "Button.action":
      return (
        <Show when={!l().words} fallback={<Status kind="value" label={l().label} />}>
          <Button {...button()} filled={l().on} {...some("glyph", r.props.glyph)} {...some("own", r.props.own)} />
        </Show>
      );
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
    case "Fader.percent":
    case "Fader.byte":
      return <Fader label={l().label} most={c.kit === "Fader.byte" ? 255 : 100} value={l().value} {...off()} onChange={(value) => props.act({ is: "set", value, value2: l().value2 })} />;
    case "XY.position":
    case "XY.colour":
      return <XY kind={c.kit === "XY.colour" ? "colour" : "position"} label={l().label} across={l().value} up={l().value2} held={l().on} {...off()} onChange={(value, value2) => props.act({ is: "set", value, value2 })} />;
    case "Picker.tabs":
      return <Picker kind="tabs" label={c.name} options={tabs()} picked={l().picked} {...off()} onPick={pick} />;
    case "Picker.segments":
      return <Picker kind="segments" label={c.name} options={options()} picked={l().picked} {...some("size", oneOrNone(r.props.size, ["small"]))} {...some("fires", oneOrNone(r.props.fires, ["down"]))} {...off()} onPick={pick} />;
    case "Picker.pictures":
      return <Picker kind="pictures" label={c.name} options={pictures()} picked={l().picked} {...off()} onPick={pick} />;
    case "Picker.cards":
      return <Picker kind="cards" label={c.name} options={cards()} picked={l().picked} {...off()} onPick={pick} />;
    case "Picker.list":
      return <Picker kind="list" label={l().label} options={options()} picked={l().picked} {...off()} onPick={pick} />;
    case "Field.text":
    case "Field.search":
      return <Field kind={c.kit === "Field.search" ? "search" : "text"} label={l().label} value={l().picked} saves={saves} {...some("refuse", SHAPES[c.id])} {...off()} onText={(text) => props.act({ is: "text", text: SHAPES[c.id] ? tidy(text) : text })} />;
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
    case "Item.entry": {
      // The menu bar item opens its menu: the state in words, the frame counter, Open window, Stop engine.
      const opens = c.id === "menubar.item";
      const entry = (inMenu: boolean) => (
        <Item kind="entry" title={l().label} {...some("sub", l().sub)} {...some("lead", LAMPS[l().variant])} standing={opens ? "plain" : standing()} {...(opens ? { expanded: l().on } : {})} inMenu={inMenu} {...off()} onPress={press} />
      );
      // An entry of the menu is drawn in its menu.
      if (!opens)
        return (
          <div role="menu" aria-label="Bossylights">
            {entry(true)}
          </div>
        );
      const id = `menu-${c.id}`;
      return (
        <div class="c-menubar">
          {entry(false)}
          <Show when={l().on}>
            <div class="c-menu">
              <p id={`${id}-state`}>{l().sub || "Running"}</p>
              <p id={`${id}-frame`}>Frame 1 204 816</p>
              <div role="menu" aria-label={l().label} aria-describedby={`${id}-state ${id}-frame`}>
                <Item kind="entry" title="Open window" inMenu onPress={() => undefined} />
                <Item kind="entry" title="Stop engine" inMenu onPress={() => undefined} />
              </div>
            </div>
          </Show>
        </div>
      );
    }
    case "Item.chip":
      return <Item kind="chip" title={l().label} {...off()} onRemove={() => props.act({ is: "remove" })} />;
    case "Item.strip": {
      const lane = (name: string, colour: "manual" | "show" | "autopilot", runs: string): StatusProps => ({ kind: "pill", name, label: runs || "nothing", colour, fill: runs ? "solid" : "dashed" });
      const now = () => NOW[l().variant] ?? NOW[""];
      const decks = (): StatusProps[] =>
        (now()?.decks ?? []).map((deck, i) => ({ kind: "pill", name: String(i + 1), label: deck.song ? `${deck.state}: ${deck.song}` : deck.state, fill: deck.state === "Playing" ? "solid" : deck.state === "Cued" ? "outline" : "dashed" }));
      const lanes = () => [lane("Manual", "manual", now()?.manual ?? ""), lane("Show", "show", now()?.show ?? ""), lane("Autopilot", "autopilot", now()?.autopilot ?? "")];
      return (
        <Item
          kind="strip"
          title="Now"
          lead={{ kind: "flag", label: "NOW" }}
          marks={[...lanes(), ...decks()]}
          says={`Now strip. ${lanes().map((s) => `${s.name}: ${s.label}`).join(". ")}. ${decks().map((d) => `Deck ${d.name}: ${d.label}`).join(". ")}. Opens the Console`}
          {...off()}
          onPress={press}
        />
      );
    }
    case "Block.bar":
      return <Block kind="bar" label={l().label} {...some("sub", l().sub)} colour={l().colour} standing={standing()} {...off()} onPress={press} {...dragged()} {...placing()} />;
    case "Block.frame":
      return <Block kind="frame" label={l().label} {...some("sub", l().sub)} standing={standing()} {...off()} onLift={lift} onDrop={drop} onCancel={cancel} onNudge={nudge} onResize={nudge} onRemove={() => props.act({ is: "remove" })} />;
    case "Handle.edge":
    case "Handle.corner":
      return <Handle kind={c.kit === "Handle.corner" ? "corner" : "edge"} label={l().label} {...off()} onLift={lift} onDrop={drop} onCancel={cancel} onNudge={nudge} />;
  }
}
