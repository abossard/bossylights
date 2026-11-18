// Every primitive of the kit with all its kinds, states and colours side by side. The front of the catalog:
// what a control is made of, before where it is used. Each specimen is live and keeps its own small state.
import { createSignal, For, type JSX } from "solid-js";
import type { Primitive } from "../controls/spec.ts";
import { Block, Button, COLOURS, Fader, Field, Handle, Item, named, Pad, Picker, Preview, Status, XY, type ButtonProps, type Colour, type PadState, type PickerKind, type PickOption, type Signal, type Standing, type StatusFill, type ToggleState } from "../kit/index.ts";
import { EFFECTS, effects, gobos, wheelSlots } from "./drawings.tsx";

export interface Specimen {
  /** What is true of it: a kind, a state, a colour. */
  says: string;
  draw: () => JSX.Element;
}

export interface Shelf {
  title: string;
  /** What to compare along it, in one line. */
  note: string;
  /** How many stand in one line, when they form a table. Left out: as many as fit. */
  across?: number;
  specimens: readonly Specimen[];
}

const nothing = () => undefined;
const SIGNALS: readonly Signal[] = [...COLOURS, "mute", "ok", "warn"];
const FILLS: readonly StatusFill[] = ["solid", "outline", "dashed"];
const ROWS: readonly Colour[] = ["manual", "show", "autopilot"];

// ---- specimens that answer: a press changes them, nothing is sent

type Rest<P> = P extends unknown ? Omit<P, "onPress" | "state" | "kind"> : never;

function Toggle(props: Rest<Extract<ButtonProps, { kind: "toggle" }>> & { start: ToggleState }) {
  const [state, setState] = createSignal(props.start);
  return <Button {...props} kind="toggle" state={state()} onPress={() => setState(state() === "on" ? "off" : "on")} />;
}

function Pick(props: { kind: PickerKind; label: string; options: readonly PickOption[]; small?: boolean; off?: string }) {
  const [picked, setPicked] = createSignal(props.options[0]?.name ?? "");
  return <Picker kind={props.kind} label={props.label} options={props.options} picked={picked()} size={props.small ? "small" : "regular"} {...(props.off === undefined ? {} : { off: props.off })} onPick={setPicked} />;
}

function Level(props: { start: number; off?: string }) {
  const [value, setValue] = createSignal(props.start);
  return <Fader label="Master" value={value()} {...(props.off === undefined ? {} : { off: props.off })} onChange={setValue} />;
}

function Aim(props: { held?: boolean; off?: string; colour?: boolean }) {
  const [at, setAt] = createSignal(props.colour ? { across: 8, up: 80 } : { across: 62, up: 40 });
  return (
    <XY
      kind={props.colour ? "colour" : "position"}
      label={props.colour ? "Colour" : "Movers"}
      across={at().across}
      up={at().up}
      held={props.held === true}
      {...(props.off === undefined ? {} : { off: props.off })}
      onChange={(across, up) => setAt({ across, up })}
    />
  );
}

/** Tabs with an action after them. The action adds a tab, so the bar can be seen to scroll. */
function Pages(props: { many?: boolean }) {
  const [pages, setPages] = createSignal(props.many ? ["Main", "Movers", "Pixels", "Strobes", "Front truss", "Back truss", "Floor", "Haze and fans", "Specials"] : ["Main", "Movers", "Pixels"]);
  const [picked, setPicked] = createSignal("Main");
  const add = () => {
    const name = `Page ${pages().length + 1}`;
    setPages([...pages(), name]);
    setPicked(name);
  };
  return (
    <div style={{ width: "100%", "max-width": "360px", "min-width": "0" }}>
      <Picker kind="tabs" label="Console pages" options={named(pages())} picked={picked()} after={{ label: "Add page", onPress: add }} onPick={setPicked} />
    </div>
  );
}

function Steps(props: { lessOff?: string; off?: string }) {
  const [beats, setBeats] = createSignal(props.lessOff === undefined ? 4 : 1);
  return (
    <Field
      kind="steps"
      label="Measure"
      value={`${beats()} ${beats() === 1 ? "beat" : "beats"}`}
      less="÷2"
      more="×2"
      {...(beats() === 1 ? { lessOff: "1 beat is the least" } : {})}
      {...(props.off === undefined ? {} : { off: props.off })}
      onStep={(by) => setBeats(Math.max(1, by > 0 ? beats() * 2 : beats() / 2))}
    />
  );
}

const WORDS = named(["Console", "Songs", "Looks"]);
const CARDS: PickOption[] = [
  { name: "Tap", sub: "You tap the beat" },
  { name: "VirtualDJ", sub: "124.0 BPM" },
  { name: "Timecode", sub: "", off: "No timecode device" },
];

const PAD_STATES: readonly [PadState, string][] = [
  ["idle", "idle: nobody runs its look"],
  ["manual", "manual: latched by hand"],
  ["flash", "flash: a finger is down"],
  ["kept", "kept: pad off, still held until Release"],
  ["show", "show: the show runs it"],
  ["autopilot", "autopilot: the autopilot runs it"],
];

const STANDINGS: readonly Standing[] = ["plain", "picked", "lifted", "dashed", "clash"];

const lanes = (runs: boolean) => [
  { kind: "pill" as const, name: "Manual", label: runs ? "Warm wash" : "nothing", colour: "manual" as const, fill: runs ? ("solid" as const) : ("dashed" as const) },
  { kind: "pill" as const, name: "Show", label: runs ? "Chase L-R" : "nothing", colour: "show" as const, fill: runs ? ("solid" as const) : ("dashed" as const) },
  { kind: "pill" as const, name: "Autopilot", label: "nothing", colour: "autopilot" as const, fill: "dashed" as const },
];

const PLASMA = EFFECTS.Plasma?.(16) ?? (() => "#000");

export const SHELVES: Record<Primitive, readonly Shelf[]> = {
  Preview: [
    {
      title: "See it before it plays",
      note: "An effect running on a small model of the panel. Read only: it takes no press and never touches the lights. What is on the lights now is a Status, not this.",
      specimens: [
        { says: "matrix, running", draw: () => <Preview label="Plasma on the panel" columns={16} rows={8} paint={PLASMA} /> },
        { says: "still: one frame, for many side by side and for less motion", draw: () => <Preview label="Plasma, one frame" columns={16} rows={8} paint={PLASMA} still /> },
        { says: "small: inside an option of a picker", draw: () => <Preview label="Plasma, small" columns={8} rows={8} paint={EFFECTS.Plasma?.(8) ?? PLASMA} size="small" /> },
      ],
    },
    {
      title: "Four effects",
      note: "Stand-ins. An effect is a calculation: a pixel and a time in, a colour out. The same one will paint the panel.",
      specimens: Object.entries(EFFECTS).map(([name, effect]) => ({ says: name, draw: () => <Preview label={`${name}, running`} columns={12} rows={8} paint={effect(12)} /> })),
    },
  ],
  Button: [
    {
      title: "Two kinds",
      note: "An action fires once. A toggle stays until it is pressed again: off, ready (it starts with LIVE), on.",
      specimens: [
        { says: "action", draw: () => <Button label="Tap" onPress={nothing} /> },
        { says: "action, filled: the one main action", draw: () => <Button label="Add lights" filled onPress={nothing} /> },
        { says: "toggle, off", draw: () => <Toggle label="Needs review" start="off" /> },
        { says: "toggle, ready", draw: () => <Toggle label="Autopilot" sub="Waits for LIVE" colour="autopilot" start="ready" /> },
        { says: "toggle, on", draw: () => <Toggle label="Needs review" start="on" /> },
      ],
    },
    {
      title: "Colour is a prop",
      note: "One line per colour: action, action filled, toggle off, toggle on. Pink, cyan and marigold only say whose it is.",
      across: 4,
      specimens: COLOURS.flatMap((colour): Specimen[] => [
        { says: `${colour}, action`, draw: () => <Button label="Release" colour={colour} onPress={nothing} /> },
        { says: `${colour}, filled`, draw: () => <Button label="Release" colour={colour} filled onPress={nothing} /> },
        { says: `${colour}, toggle off`, draw: () => <Toggle label="Mute" colour={colour} start="off" /> },
        { says: `${colour}, toggle on`, draw: () => <Toggle label="Mute" colour={colour} start="on" /> },
      ]),
    },
    {
      title: "A toggle with a mark",
      note: "A lamp, a chevron or a knob shows the state beside the fill. Quiet: the mark alone shows it.",
      across: 3,
      specimens: [
        { says: "lamp, quiet: off", draw: () => <Toggle label="Lock" mark="lamp" quiet start="off" /> },
        { says: "lamp, quiet: ready", draw: () => <Toggle label="Lock" mark="lamp" quiet start="ready" /> },
        { says: "lamp, quiet: on", draw: () => <Toggle label="Lock" mark="lamp" quiet start="on" /> },
        { says: "lamp, red, quiet: off", draw: () => <Toggle label="Recording" mark="lamp" colour="danger" quiet start="off" /> },
        { says: "lamp, red, quiet: ready", draw: () => <Toggle label="Recording" mark="lamp" colour="danger" quiet start="ready" /> },
        { says: "lamp, red, quiet: on", draw: () => <Toggle label="Recording" mark="lamp" colour="danger" quiet start="on" /> },
        { says: "lamp, marigold: off", draw: () => <Toggle label="Autopilot" mark="lamp" colour="autopilot" start="off" /> },
        { says: "lamp, marigold: ready", draw: () => <Toggle label="Autopilot" mark="lamp" colour="autopilot" start="ready" /> },
        { says: "lamp, marigold: on is a fill", draw: () => <Toggle label="Autopilot" mark="lamp" colour="autopilot" start="on" /> },
        { says: "lamp ok: linked", draw: () => <Toggle label="DJ link" mark="lamp" quiet lamp="ok" start="on" /> },
        { says: "lamp warn: searching", draw: () => <Toggle label="DJ link" mark="lamp" quiet lamp="warn" start="on" /> },
        { says: "lamp danger: lost", draw: () => <Toggle label="DJ link" mark="lamp" quiet lamp="danger" start="on" /> },
        { says: "chevron: folded", draw: () => <Toggle label="Washes" sub="4" mark="chevron" quiet start="off" /> },
        { says: "chevron: unfolded", draw: () => <Toggle label="Washes" sub="4" mark="chevron" quiet start="on" /> },
        { says: "chevron: something is dragged over it", draw: () => <Toggle label="Washes" sub="4" mark="chevron" quiet over start="off" /> },
        { says: "knob, large: off", draw: () => <Toggle label="LIVE off" mark="knob" size="large" holds={["LIVE off", "LIVE on"]} start="off" /> },
        { says: "knob, large: on", draw: () => <Toggle label="LIVE on" mark="knob" size="large" holds={["LIVE off", "LIVE on"]} start="on" /> },
      ],
    },
    {
      title: "Sizes, signs, its own colour",
      note: "Small is 30 px: an expert screen, or inside another part. A sign stands for the words, the words are still read out.",
      specimens: [
        { says: "regular, 44 px", draw: () => <Button label="Undo" sub="Move widget" onPress={nothing} /> },
        { says: "small, 30 px", draw: () => <Button label="Release" size="small" onPress={nothing} /> },
        { says: "a sign", draw: () => <Button label="Take Warm wash out" glyph="×" onPress={nothing} /> },
        { says: "a sign, small", draw: () => <Button label="Take Warm wash out" glyph="×" size="small" onPress={nothing} /> },
        { says: "its own colour: add", draw: () => <Button label="Add amber" glyph="+" own="#ffb300" onPress={nothing} /> },
        { says: "its own colour: take out", draw: () => <Button label="Take red out" glyph="×" own="#ff3b30" onPress={nothing} /> },
      ],
    },
    {
      title: "Off, mapped, a width that holds",
      note: "Off is dashed and says why. A mapped control has a paper square. A button that changes its words keeps the width of the widest.",
      specimens: [
        { says: "off, with the reason", draw: () => <Button label="Edit surface" off="Locked while LIVE is on" onPress={nothing} /> },
        { says: "a toggle, off with the reason", draw: () => <Button kind="toggle" state="off" label="Recording" mark="lamp" quiet off="Engine not connected" onPress={nothing} /> },
        { says: "mapped to a hardware control", draw: () => <Button label="Tap" mapped onPress={nothing} /> },
        { says: "holds its width: off", draw: () => <Button label="Blackout" colour="danger" holds={["Blackout", "Blackout on", "While Space is down"]} onPress={nothing} /> },
        { says: "holds its width: on", draw: () => <Button label="Blackout on" sub="While Space is down" colour="danger" filled holds={["Blackout", "Blackout on", "While Space is down"]} onPress={nothing} /> },
      ],
    },
  ],
  Pad: [
    {
      title: "Who runs it",
      note: "The body takes the colour of who started the look. The NOW flag says it runs. A finger down is a paper frame.",
      specimens: [
        ...PAD_STATES.map(([state, says]): Specimen => ({ says, draw: () => <Pad label="Warm wash" colours={["#ff9d2e", "#ffd27a"]} state={state} onDown={nothing} onUp={nothing} /> })),
        { says: "off, with the reason", draw: () => <Pad label="Warm wash" colours={["#ff9d2e", "#ffd27a"]} state="idle" off="Engine not connected" onDown={nothing} onUp={nothing} /> },
        { says: "mapped", draw: () => <Pad label="Chase L-R" colours={["#20b2c9"]} state="idle" mapped onDown={nothing} onUp={nothing} /> },
      ],
    },
  ],
  Fader: [
    {
      title: "One value",
      note: "The thumb follows the finger at once. The number is what came back.",
      specimens: [
        { says: "at 0", draw: () => <Level start={0} /> },
        { says: "at 62", draw: () => <Level start={62} /> },
        { says: "at 100", draw: () => <Level start={100} /> },
        { says: "off, with the reason", draw: () => <Level start={62} off="Engine not connected" /> },
      ],
    },
  ],
  XY: [
    {
      title: "Two values",
      note: "A paper crosshair. Pink while the position is held by hand.",
      specimens: [
        { says: "free", draw: () => <Aim /> },
        { says: "held by hand", draw: () => <Aim held /> },
        { says: "off, with the reason", draw: () => <Aim off="No moving lights" /> },
      ],
    },
    {
      title: "Two kinds",
      note: "A position is a plane. A colour is a wheel: the hue around, its strength outward, white in the middle. Arrows: left and right turn the hue, up and down the strength.",
      specimens: [
        { says: "position", draw: () => <Aim /> },
        { says: "colour", draw: () => <Aim colour /> },
        { says: "colour, off", draw: () => <Aim colour off="This light has a colour wheel" /> },
      ],
    },
  ],
  Picker: [
    {
      title: "Five kinds",
      note: "All of them pick one of a few. Which kind depends on how many options there are and how much room.",
      specimens: [
        { says: "tabs", draw: () => <Pick kind="tabs" label="Workspace" options={WORDS} /> },
        { says: "segments", draw: () => <Pick kind="segments" label="Speed" options={named(["half", "normal", "double"])} /> },
        { says: "cards: a line under each", draw: () => <Pick kind="cards" label="Tempo feed" options={CARDS.slice(0, 2)} /> },
        { says: "pictures: known by their look", draw: () => <Pick kind="pictures" label="Gobo" options={gobos(["Open", "Dots", "Star", "Bars"])} /> },
        { says: "list: drawn by the kit, never by the system", draw: () => <Pick kind="list" label="Style" options={named(["House", "Techno", "Chill", "Drum and bass"])} /> },
      ],
    },
    {
      title: "States",
      note: "An option that is off says why. A picker that is off says why under it. A red square: something behind it has trouble.",
      specimens: [
        { says: "tabs, one marked", draw: () => <Pick kind="tabs" label="Workspace" options={[...WORDS.slice(0, 2), { name: "In/Out", marked: true }]} /> },
        { says: "segments, small", draw: () => <Pick kind="segments" label="Order" options={named(["HTP", "LTP"])} small /> },
        { says: "segments, off", draw: () => <Pick kind="segments" label="Speed" options={named(["half", "normal", "double"])} off="No song plays" /> },
        { says: "cards, one option off", draw: () => <Pick kind="cards" label="Tempo feed" options={CARDS} /> },
        { says: "list, one option off", draw: () => <Pick kind="list" label="Device" options={[...named(["APC mini", "Launchpad"]), { name: "nanoKONTROL", off: "Not plugged in" }]} /> },
        { says: "list, off", draw: () => <Pick kind="list" label="Style" options={named(["House"])} off="Engine not connected" /> },
      ],
    },
    {
      title: "Pictures: three choosers, one kind",
      note: "A gobo chooser, the colour wheel of a light and a matrix effect chooser are the same control: one of a few, known by its look. Only the picture differs.",
      specimens: [
        { says: "gobo chooser", draw: () => <Pick kind="pictures" label="Gobo" options={gobos(["Open", "Dots", "Star", "Bars"])} /> },
        { says: "colour wheel of a light: its slots, one split", draw: () => <Pick kind="pictures" label="Colour wheel" options={wheelSlots()} /> },
        { says: "matrix effect chooser: a Preview runs in each option", draw: () => <Pick kind="pictures" label="Effect" options={effects()} /> },
        { says: "one option off", draw: () => <Pick kind="pictures" label="Gobo" options={[...gobos(["Open", "Dots"]), { ...gobos(["Star"])[0], name: "Star", off: "Not on this wheel" }]} /> },
        { says: "off", draw: () => <Pick kind="pictures" label="Gobo" options={gobos(["Open", "Dots", "Star"])} off="This light has no gobos" /> },
      ],
    },
    {
      title: "The tab bar",
      note: "Tabs that do not fit scroll sideways and the picked one stays in view. One action may follow the tabs: a button with a dashed edge, never a tab. Press it to see the bar grow.",
      specimens: [
        { says: "tabs with an action after them", draw: () => <Pages /> },
        { says: "more tabs than fit: it scrolls", draw: () => <Pages many /> },
      ],
    },
  ],
  Field: [
    {
      title: "Three kinds",
      note: "Text sends on Enter, Escape puts the old text back. Search sends with every letter. Steps: one less, one more.",
      specimens: [
        { says: "text", draw: () => <Field kind="text" label="Section name" value="Drop" onText={nothing} /> },
        { says: "search", draw: () => <Field kind="search" label="Search songs" value="" placeholder="title or artist" onText={nothing} /> },
        { says: "steps", draw: () => <Steps /> },
        { says: "steps, one side off", draw: () => <Steps lessOff="" /> },
        { says: "text, off", draw: () => <Field kind="text" label="Section name" value="Drop" off="Locked while LIVE is on" onText={nothing} /> },
        { says: "steps, off", draw: () => <Steps off="Engine not connected" /> },
      ],
    },
  ],
  Status: [
    {
      title: "Lamp",
      note: "One line per colour: lit, empty, struck out. Green and orange are for lamps only.",
      across: 3,
      specimens: SIGNALS.flatMap((colour) => FILLS.map((fill): Specimen => ({ says: `${colour}, ${fill}`, draw: () => <Status kind="lamp" label="DJ link" colour={colour} fill={fill} /> }))),
    },
    {
      title: "Pill",
      note: "One line per colour: it is so now, it is so but not in effect, not now. With a name: a lane.",
      across: 3,
      specimens: [
        ...COLOURS.flatMap((colour) => FILLS.map((fill): Specimen => ({ says: `${colour}, ${fill}`, draw: () => <Status kind="pill" label="Playing" colour={colour} fill={fill} /> }))),
        ...ROWS.map((colour): Specimen => ({ says: `${colour}, with its name`, draw: () => <Status kind="pill" name={colour.charAt(0).toUpperCase() + colour.slice(1)} label="Warm wash" colour={colour} /> })),
      ],
    },
    {
      title: "Flag and value",
      note: "The flag is paper: now. A value is a reading in the display face, with its name above when its place does not say.",
      specimens: [
        { says: "flag", draw: () => <Status kind="flag" label="NOW" /> },
        { says: "flag, with the time", draw: () => <Status kind="flag" label="NOW 22:47" /> },
        { says: "value", draw: () => <Status kind="value" label="62" /> },
        { says: "value, with its name", draw: () => <Status kind="value" name="Fade" label="1 beat" /> },
        { says: "value, mute: its control is off", draw: () => <Status kind="value" label="62" colour="mute" /> },
      ],
    },
  ],
  Item: [
    {
      title: "Row",
      note: "One line of a list. Picked is paper. Its state is a pill at the right, its own colour a field at the left.",
      specimens: [
        { says: "plain", draw: () => <Item kind="row" title="Night Drive" sub="124 BPM, 6 sections" onPress={nothing} /> },
        { says: "picked", draw: () => <Item kind="row" title="Night Drive" sub="124 BPM, 6 sections" standing="picked" onPress={nothing} /> },
        { says: "with a state", draw: () => <Item kind="row" title="Night Drive" sub="124 BPM" marks={[{ kind: "pill", label: "Playing", colour: "paper" }]} onPress={nothing} /> },
        { says: "with trouble", draw: () => <Item kind="row" title="Universe 1" sub="15 lights" marks={[{ kind: "pill", label: "Clash", colour: "danger", fill: "outline" }]} onPress={nothing} /> },
        { says: "with its own colour", draw: () => <Item kind="row" title="Warm wash" sub="Step" swatch="#ff9d2e" onPress={nothing} /> },
        { says: "lifted: being dragged", draw: () => <Item kind="row" title="Warm wash" sub="Step" standing="lifted" onPress={nothing} /> },
        { says: "off, with the reason", draw: () => <Item kind="row" title="DMX USB Pro" off="Engine not connected" onPress={nothing} /> },
      ],
    },
    {
      title: "Tile, cell, entry, chip",
      note: "The same thing in other places: a box in a picture, a square of a grid, a line of a menu, a thing that can be taken out.",
      specimens: [
        { says: "tile", draw: () => <Item kind="tile" title="Chase L-R" sub="Sequence" onPress={nothing} /> },
        { says: "tile, picked", draw: () => <Item kind="tile" title="Chase L-R" sub="Sequence" standing="picked" onPress={nothing} /> },
        { says: "cell", draw: () => <Item kind="cell" title="PAR 3 red" sub="17" marks={[{ kind: "value", label: "128" }]} onPress={nothing} /> },
        { says: "cell, picked", draw: () => <Item kind="cell" title="PAR 3 red" sub="17" marks={[{ kind: "value", label: "128" }]} standing="picked" onPress={nothing} /> },
        { says: "cell, held by hand", draw: () => <Item kind="cell" title="PAR 3 red" sub="17" marks={[{ kind: "value", label: "255" }]} colour="manual" onPress={nothing} /> },
        { says: "cell, changing", draw: () => <Item kind="cell" title="PAR 3 red" sub="17" marks={[{ kind: "value", label: "64" }]} standing="dashed" onPress={nothing} /> },
        { says: "cell, clash", draw: () => <Item kind="cell" title="PAR 3 red" sub="17" marks={[{ kind: "value", label: "128" }]} standing="clash" onPress={nothing} /> },
        { says: "entry", draw: () => <Item kind="entry" title="Open window" onPress={nothing} /> },
        { says: "entry, off", draw: () => <Item kind="entry" title="Stop engine" off="LIVE is on" onPress={nothing} /> },
        { says: "entry with a lamp: the menu bar item", draw: () => <Item kind="entry" title="Bossylights" sub="Running" lead={{ kind: "lamp", label: "Engine runs", colour: "mute" }} onPress={nothing} /> },
        { says: "chip: the cross takes it out", draw: () => <Item kind="chip" title="Warm wash" onRemove={nothing} /> },
      ],
    },
    {
      title: "Strip",
      note: "A full line of statuses, pressed as one. Its lanes share the width and stack when it gets narrow.",
      across: 1,
      specimens: [
        { says: "something runs", draw: () => <Item kind="strip" title="Now" lead={{ kind: "flag", label: "NOW" }} marks={lanes(true)} onPress={nothing} /> },
        { says: "nothing runs", draw: () => <Item kind="strip" title="Now" lead={{ kind: "flag", label: "NOW" }} marks={lanes(false)} onPress={nothing} /> },
      ],
    },
  ],
  Block: [
    {
      title: "Bar",
      note: "One line per colour: plain, picked, lifted, dashed, clash. The last line has a colour of its own.",
      across: 5,
      specimens: [
        ...(["plain", ...ROWS] as const).flatMap((colour) => STANDINGS.map((standing): Specimen => ({ says: `${colour}, ${standing}`, draw: () => <Block kind="bar" label="Drop" sub="16 bars" colour={colour} standing={standing} onPress={nothing} /> }))),
        ...STANDINGS.map((standing): Specimen => ({ says: `own colour, ${standing}`, draw: () => <Block kind="bar" label="PAR 3" sub="17 to 23" own="#b48cff" standing={standing} onPress={nothing} /> })),
      ],
    },
    {
      title: "Frame",
      note: "Over a widget while the surface is edited. Drag moves it, arrows move it, Shift and arrows resize it, Delete removes it.",
      specimens: [
        { says: "plain", draw: () => <Block kind="frame" label="Pads" sub="4 x 3" /> },
        { says: "lifted", draw: () => <Block kind="frame" label="Pads" sub="4 x 3" standing="lifted" /> },
      ],
    },
  ],
  Handle: [
    {
      title: "Two kinds",
      note: "Only dragged, or nudged with the arrows. Nothing is sent before it is dropped.",
      specimens: [
        { says: "edge", draw: () => <Handle kind="edge" label="Edge between Build and Drop" onDrop={nothing} onNudge={nothing} /> },
        { says: "corner", draw: () => <Handle kind="corner" label="Corner of Pads" onDrop={nothing} onNudge={nothing} /> },
      ],
    },
  ],
};

/** One shelf: its specimens side by side, each with what is true of it under it. */
export function ShelfView(props: { shelf: Shelf }) {
  return (
    <section class="c-shelf" data-shelf={props.shelf.title}>
      <h2>{props.shelf.title}</h2>
      <p class="c-fact">{props.shelf.note}</p>
      <div class="c-specimens" style={props.shelf.across ? { "grid-template-columns": `repeat(${props.shelf.across}, minmax(0, 1fr))` } : {}}>
        <For each={props.shelf.specimens}>
          {(specimen) => (
            <figure class="c-specimen" data-specimen>
              <div class="c-stage">{specimen.draw()}</div>
              <figcaption>{specimen.says}</figcaption>
            </figure>
          )}
        </For>
      </div>
    </section>
  );
}
