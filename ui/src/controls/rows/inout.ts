// In/Out: the tempo, the outputs, and the list of what was mapped with Learn.
import { off, works, type Cell, type SituationId } from "../situations.ts";
import { button, change, choice, control, field, remove, select, toggle, LOCKED, NOT_A_BUTTON, NOT_A_LIST, UNIVERSES, no, type Control, type Row } from "../spec.ts";

const LIVE_IS_ON = off("LIVE is on");
const ROW = no("It belongs to one row of a list that changes");

/** A control of one row of the Mappings table. The lock holds mappings, as it holds the layout (decision 21). */
const MAPPING: Partial<Record<SituationId, Cell>> = { lock: LOCKED, empty: works("Works. Top bar controls can be mapped before any light is patched") };
const mapping = (row: Omit<Row, "screen" | "kind" | "part">): Control =>
  control({ learn: ROW, ...row, screen: "In/Out", part: "Mappings", kind: "edit", in: { ...MAPPING, ...row.in } });

/** The rates each kind of output takes, in frames a second. The DMX USB Pro sends at most 44. */
export const RATES = { DDP: ["30", "44", "60"], "DMX USB Pro": ["30", "44"] } as const;

/** The rate picker of each kind of output. */
export const RATE_PICKERS = [
  { id: "inout.out.rate", kind: "DDP" },
  { id: "inout.out.rate-usb", kind: "DMX USB Pro" },
] as const;

const ON_SCREEN = no("Outputs and inputs are set up on screen");
/** A control that sets up an output: not while LIVE is on, the lights hang on it. */
const output = (row: Omit<Row, "screen" | "kind" | "part">): Control =>
  control({
    learn: ON_SCREEN,
    ...row,
    screen: "In/Out",
    part: "Out",
    kind: "setting",
    in: {
      "live-on": LIVE_IS_ON,
      lock: LIVE_IS_ON,
      recording: LIVE_IS_ON,
      empty: works("Works. An output is set up before any light is patched"),
      "output-lost": works("Works. This is how a lost output gets another device"),
      ...row.in,
    },
  });
/** A control of the In column. */
const input = (row: Omit<Row, "screen" | "kind">): Control => control({ learn: ON_SCREEN, ...row, screen: "In/Out", kind: "setting" });

export const INOUT: Control[] = [
  control({
    id: "inout.tempo.feed",
    name: "Tempo feed",
    screen: "In/Out",
    part: "Tempo",
    from: ["inout/Screen/button"],
    kit: "Picker.cards",
    kind: "setting",
    does: "Picks what drives the beat: Tap, Audio, MIDI clock, VirtualDJ or Timecode. Each says its tempo and how sure it is. While taps hold the beat the picked feed waits: 'from Tap' in the top bar gives it back",
    sends: ["SetTempoFeed"],
    gestures: choice("The beat follows that feed"),
    in: {
      "live-on": works("Works mid-show. The beat jumps to the new feed at once"),
      song: works("Works. VirtualDJ gives the tempo of the song that plays"),
      "no-song": works("Works. VirtualDJ has no tempo without a song: the last tempo stays"),
    },
    first: change("A feed with no signal can be picked, and then nothing drives the beat. It is off and says 'No signal'"),
    states: ["picked", "not picked", "off: no signal"],
  }),
  control({
    id: "inout.follows",
    name: "Follows",
    screen: "In/Out",
    part: "Tempo",
    from: ["songs/Screen/follows"],
    kit: "Picker.segments",
    kind: "setting",
    does: "Which deck, or timecode, gives the song that plays and its position. Beside the tempo feed, which gives the beat",
    sends: ["SetFollows"],
    gestures: choice("Follows that source"),
    in: {
      song: works("Works. The Show row jumps to the song of that source, at once"),
      "no-song": works("Works. It counts from the next song on"),
      empty: works("Works. No song has a show yet: it counts once one has"),
    },
    first: change("In the mockup it sits in the header of one song. The owner's decision 26: it is a setting of the whole app and sits on In/Out, beside the tempo feed. DJ link stays the on or off"),
    states: ["deck 1", "deck 2", "timecode"],
  }),
  control({
    id: "inout.tempo.tap",
    name: "Tap, on In/Out",
    screen: "In/Out",
    part: "Tempo",
    from: ["inout/Screen/tap"],
    kit: "Button.action",
    kind: "play",
    does: "The same as Tap in the top bar",
    sends: ["TapTempo"],
    gestures: button("One tap"),
    first: remove("Two controls for one thing. Tap is in the top bar on every screen, right above this one. The Tap feed says 'Tap in the top bar'"),
  }),
  control({
    id: "inout.out.test",
    name: "Test an output",
    screen: "In/Out",
    part: "Out",
    from: ["inout/Screen/testing"],
    kit: "Button.toggle",
    kind: "play",
    does: "Nothing: it goes. To find a light, Identify it",
    sends: [],
    gestures: button("Nothing"),
    learn: no("It goes"),
    first: remove("Identify only: no test drives every channel of an output. Haze, strobe and sound never move unless a look or a hand sets them"),
  }),
  mapping({
    id: "inout.mapping.edit",
    name: "Edit a mapping",
    from: ["inout/Screen/button#2"],
    kit: "Button.toggle",
    does: "Turns device, number and feedback of that row into fields. Pressed again as 'Done', they are text again",
    ui: "which row is being edited",
    gestures: toggle("Opens the row to edit", "the row is text again"),
    undo: no("Opening a row changes nothing"),
    states: ["Edit", "Done"],
  }),
  mapping({
    id: "inout.mapping.device",
    name: "Mapping device",
    from: ["inout/Screen/device-for"],
    kit: "Picker.list",
    does: "Which controller the mapping listens to. There while the row is edited",
    sends: ["UpdateMapping"],
    gestures: select("the mapping listens to that device"),
    learn: NOT_A_LIST,
  }),
  mapping({
    id: "inout.mapping.message",
    name: "Note or controller number",
    from: ["inout/Screen/note-or-controller-number-for"],
    kit: "Field.text",
    does: "The note or controller number the mapping listens to. There while the row is edited. An empty field keeps the old one",
    sends: ["UpdateMapping"],
    gestures: field("a note or a controller number", "Enter or leaving the field saves, Escape puts the old one back"),
    learn: NOT_A_BUTTON,
    first: change("Free text for something that has a shape: 'Note 36', 'CC 48'. Text that is not one of the two is refused and the field says so"),
  }),
  mapping({
    id: "inout.mapping.feedback",
    name: "Mapping feedback",
    from: ["inout/Screen/lights-up"],
    kit: "Button.toggle",
    does: "The hardware control lights up with the state of the control on screen, or not. There while the row is edited",
    sends: ["UpdateMapping"],
    gestures: toggle("The hardware control lights up", "it stays dark"),
    states: ["Lights up", "None"],
  }),
  mapping({
    id: "inout.mapping.test",
    name: "Test a mapping",
    from: ["inout/Screen/test"],
    kit: "Button.action",
    does: "Shows that the mapping is alive: the hardware control lights and the line under the table says what was sent. The control on screen does not fire",
    sends: ["TestMapping"],
    gestures: button("Tests the mapping"),
    in: { lock: works("Works. A test changes nothing and fires nothing") },
    undo: no("A test changes nothing"),
    states: ["Test", "Sent"],
  }),
  mapping({
    id: "inout.mapping.remove",
    name: "Remove a mapping",
    from: ["inout/Screen/remove"],
    kit: "Button.action",
    does: "Removes the mapping. The control on screen loses its paper square",
    sends: ["RemoveMapping"],
    gestures: button("Removes the mapping"),
  }),
  // ---- in the mockups since review 4 (decision 29)
  control({
    id: "inout.out.setup",
    name: "Set up an output",
    screen: "In/Out",
    part: "Out",
    from: ["inout/OutputSetup/set-up-the-output"],
    kit: "Button.toggle",
    kind: "view",
    does: "Opens one output to set up: its kind, universes, rate, and Remove. Pressed again as 'Done', they are text again",
    ui: "which output is being set up",
    gestures: toggle("Opens the output to set up", "it is text again"),
    learn: ON_SCREEN,
    states: ["Set up", "Done"],
  }),
  output({
    id: "inout.out.kind",
    name: "Kind of an output",
    from: ["inout/OutputSetup/kind-of-the-output"],
    kit: "Picker.list",
    does: "What the output is: DMX USB Pro, DDP, Art-Net or sACN. There while the output is set up",
    sends: ["SetOutput"],
    gestures: select("the output sends as that kind"),
    learn: NOT_A_LIST,
  }),
  output({
    id: "inout.out.universes",
    name: "Universes of an output",
    from: ["inout/OutputSetup/universes-of-the-output"],
    kit: "Field.text",
    does: "Which universes the output carries: '1', or '2 to 9'.",
    sends: ["SetOutput"],
    gestures: field(`one universe or a range, each from 1 to ${UNIVERSES}`, "Enter or leaving the field saves, Escape puts the old one back"),
    learn: NOT_A_BUTTON,
  }),
  // One rate picker per kind of output: each offers only the rates that kind takes.
  ...RATE_PICKERS.map(({ id, kind }) =>
    output({
      id,
      name: `Frames a second, ${kind}`,
      from: ["inout/OutputSetup/frames-a-second"],
      kit: "Picker.segments",
      does: `How often a ${kind} output sends: ${RATES[kind].join(", ")} frames a second. Each output has its own rate`,
      sends: ["SetOutput"],
      gestures: choice("The output sends at that rate"),
      states: [...RATES[kind]],
    }),
  ),
  output({
    id: "inout.out.remove",
    name: "Remove output",
    from: ["inout/OutputSetup/remove-the-output"],
    kit: "Button.action",
    does: "Removes the output. Its universes send nowhere until another output carries them",
    sends: ["RemoveOutput"],
    gestures: button("Removes the output"),
  }),
  output({
    id: "inout.out.add",
    name: "Add output",
    from: ["inout/AddOutput/add-an-output"],
    kit: "Button.action",
    does: "Adds an output that is not set up yet and opens it. It sends nothing until it has a universe",
    sends: ["SetOutput"],
    gestures: button("Adds an output"),
    in: { "live-on": works("Works mid-show: a new output takes nothing from the ones that run"), lock: works("Works the same"), recording: works("Works the same") },
  }),
  input({
    id: "inout.audio.input",
    name: "Audio input",
    part: "In, Audio input",
    from: ["inout/AudioInput/audio-input"],
    kit: "Picker.list",
    does: "Which input the beat is heard on. The meter beside it shows what arrives",
    sends: ["SetAudioInput"],
    gestures: select("the beat is heard on that input"),
    in: { "live-on": works("Works mid-show. With Audio as the tempo feed the beat is found again within a few bars") },
    learn: NOT_A_LIST,
  }),
  input({
    id: "inout.osc.on",
    name: "OSC on or off",
    part: "In, OSC",
    from: ["inout/Osc/osc-on-or-off"],
    kit: "Button.toggle",
    does: "Other software may send commands to this machine over OSC, or not. Off from the start",
    sends: ["SetOsc"],
    gestures: toggle("OSC listens", "it is off"),
    states: ["Off", "Listening"],
  }),
  input({
    id: "inout.osc.port",
    name: "OSC port",
    part: "In, OSC",
    from: ["inout/Osc/osc-port"],
    kit: "Field.text",
    does: "The port OSC listens on. A port that is taken is refused and the field says so",
    sends: ["SetOsc"],
    gestures: field("a port number", "Enter or leaving the field saves, Escape puts the old one back"),
    learn: NOT_A_BUTTON,
  }),
];
