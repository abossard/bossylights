// One control, live: the kit component of its row, drawn from a `Look`, its gestures reported as acts.
// The only place that knows which component draws which kit.
import type { JSX } from "solid-js";
import type { Control } from "../controls/spec.ts";
import { Bar, Button, Cell, Chip, Choice, Edge, Fader, Fold, Indicator, LiveSwitch, MenuItem, Node, NowStrip, Pad, Row, Segmented, Select, Stepper, Swatch, Tabs, TextField, Toggle, WidgetFrame, XYPad } from "../kit/index.ts";
import type { Act } from "./act.ts";
import type { Look } from "./look.ts";
import { USES } from "./uses.ts";

/** A press that comes up within this is short. Longer, it was a hold (the pad machine). */
const SHORT_PRESS_MS = 350;

const oneOf = <const T extends string>(value: string, all: readonly T[], otherwise: T): T => all.find((v) => v === value) ?? otherwise;
const some = (text: string) => (text === "" ? undefined : text);

export function Live(props: { control: Control; look: Look; act: (act: Act) => void }): JSX.Element {
  const c = props.control;
  const l = () => props.look;
  const press = () => props.act({ is: "press" });
  /** The whole control is off, with its reason. */
  const off = () => (l().off && !l().offOption ? l().off : undefined);
  const sideOff = (option: string | undefined) => (l().off && option !== undefined && l().offOption === option ? l().off : undefined);
  const fires = c.gestures.press.includes("on the way down") ? "down" : "up";
  const drags = USES[c.id]?.machine === "drag";
  const lift = () => props.act({ is: "lift" });
  const drop = () => props.act({ is: "drop" });
  const cancel = () => props.act({ is: "cancel" });
  /** A step from the keyboard: lifted and dropped at once. */
  const nudge = () => {
    lift();
    drop();
  };

  switch (c.kit) {
    case "Button":
      return <Button label={l().label} sub={some(l().sub)} tone={l().tone} on={l().on} fires={fires} off={off()} onPress={press} />;
    case "Toggle":
      return <Toggle label={l().label} sub={some(l().sub)} on={l().on} tone={l().tone === "plain" ? "paper" : l().tone} off={off()} onToggle={press} />;
    case "Pad":
      return (
        <Pad
          label={l().label}
          colours={["#ff9d2e", "#ffd27a"]}
          state={oneOf(l().variant, ["idle", "manual", "flash", "kept", "show", "autopilot"], "idle")}
          off={off()}
          onDown={() => props.act({ is: "down" })}
          onUp={(heldMs) => props.act({ is: "up", long: heldMs > SHORT_PRESS_MS })}
        />
      );
    case "Fader":
      return <Fader label={l().label} value={l().value} off={off()} onChange={(value) => props.act({ is: "set", value, value2: l().value2 })} />;
    case "XYPad":
      return <XYPad label={l().label} pan={l().value} tilt={l().value2} held={l().on} off={off()} onChange={(value, value2) => props.act({ is: "set", value, value2 })} />;
    case "Tabs":
      return <Tabs label={c.name} options={l().options} selected={l().picked} marked={l().variant === "marked" ? l().options.slice(1, 2) : []} onPick={(option) => props.act({ is: "pick", option })} />;
    case "Segmented":
      return <Segmented label={c.name} options={l().options} selected={l().picked} small={c.gestures.touch.includes("30 px")} off={off()} onPick={(option) => props.act({ is: "pick", option })} />;
    case "Choice":
      return <Choice label={c.name} options={l().options.map((name) => ({ name, sub: "", ...(sideOff(name) === undefined ? {} : { off: sideOff(name) }) }))} selected={l().picked} onPick={(option) => props.act({ is: "pick", option })} />;
    case "Select":
      return <Select label={l().label} options={l().options} value={l().picked} off={off()} onPick={(option) => props.act({ is: "pick", option })} />;
    case "TextField":
      return <TextField label={l().label} value={l().picked} live={c.kind === "view"} off={off()} onText={(text) => props.act({ is: "text", text })} />;
    case "Stepper":
      return (
        <Stepper
          label={l().label}
          value={l().sub}
          less={l().options[0] ?? "Less"}
          more={l().options[1] ?? "More"}
          lessOff={off() ?? sideOff(l().options[0])}
          moreOff={off() ?? sideOff(l().options[1])}
          onStep={(by) => props.act({ is: "step", by })}
        />
      );
    case "Row":
      return (
        <Row
          title={l().label}
          sub={some(l().sub)}
          mark={/^[A-Z]/.test(l().variant) ? l().variant : undefined}
          markTone={l().tone}
          state={l().variant === "lifted" ? "lifted" : l().on ? "selected" : "plain"}
          off={off()}
          onPress={press}
          {...(drags ? { onLift: lift, onDrop: drop, onCancel: cancel } : {})}
        />
      );
    case "Chip":
      return <Chip label={l().label} onRemove={() => props.act({ is: "remove" })} />;
    case "Bar":
      return (
        <Bar
          label={l().label}
          sub={some(l().sub)}
          tone={l().tone}
          state={l().on ? "selected" : oneOf(l().variant, ["dashed", "lifted", "clash"], "plain")}
          onPress={press}
          {...(drags ? { onLift: lift, onDrop: drop, onCancel: cancel } : {})}
        />
      );
    case "Edge":
      return <Edge label={l().label} corner={c.id.endsWith("resize")} onLift={lift} onDrop={drop} onCancel={cancel} onNudge={nudge} />;
    case "Cell":
      return <Cell address={17} name={l().label} value={l().value} state={l().on ? "selected" : oneOf(l().variant, ["held", "changing", "clash"], "plain")} onPress={press} />;
    case "Swatch":
      return <Swatch label={l().label} colour={l().variant === "remove" ? "#ff3b30" : "#ffb300"} does={l().variant === "remove" ? "remove" : "add"} off={off()} onPress={press} />;
    case "NowStrip":
      return (
        <NowStrip
          lanes={[
            { name: "Manual", tone: "manual", runs: l().on ? ["Warm wash"] : [] },
            { name: "Show", tone: "show", runs: l().on ? ["Chase L-R"] : [] },
            { name: "Autopilot", tone: "autopilot", runs: [] },
          ]}
          onPress={press}
        />
      );
    case "Fold":
      return <Fold label={l().label} count={l().value} open={l().on} over={l().variant === "over"} onToggle={press} />;
    case "WidgetFrame":
      return <WidgetFrame name={l().label} size={l().sub} cell={48} lifted={l().variant === "lifted"} onMove={nudge} onResize={nudge} onRemove={() => props.act({ is: "remove" })} onLift={lift} onCancel={cancel} />;
    case "Node":
      return <Node label={l().label} kind={l().sub} open={l().on} onPress={press} />;
    case "LiveSwitch":
      return <LiveSwitch on={l().on} off={off()} onToggle={press} />;
    case "Indicator":
      return <Indicator label={l().label} state={oneOf(l().variant, ["ready", "on", "off"], "ready")} danger={c.id.endsWith("recording")} off={off()} onPress={press} />;
    case "MenuItem":
      return (
        <MenuItem
          label={l().label}
          is={c.id === "menubar.item" ? "item" : "entry"}
          lamp={c.id === "menubar.item" ? oneOf(l().variant, ["mute", "paper", "danger", "empty"], "mute") : undefined}
          sub={some(l().sub)}
          open={l().on}
          off={off()}
          onPress={press}
        />
      );
  }
}
