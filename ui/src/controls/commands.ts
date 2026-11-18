// The commands the controls send: the union is the first list of engine commands (issue #102).
// Names follow `engine-api.ts`. `engine: true` means the engine already accepts it today.
// This is the list the UI needs, not a design of the engine: fields and names are settled per issue.

interface CommandSpec {
  /** What it does, in plain words. */
  does: string;
  /** Its fields. */
  fields: string;
  /** The engine accepts it today. */
  engine?: true;
}

const command = (does: string, fields = "", engine?: true): CommandSpec => (engine ? { does, fields, engine } : { does, fields });

export const COMMANDS = {
  // ---- the night
  SetLive: command("The night starts or ends", "on"),
  SetFollowDj: command("Follow the DJ software while LIVE is on, or not", "on"),
  SetRecording: command("Record what is played while LIVE is on, or not", "on"),
  SetAutopilot: command("The autopilot plays, or not", "on"),
  SetAutopilotStyle: command("Which style the autopilot uses", "style"),
  SetLock: command("Lock the Console layout while LIVE is on, or not", "on"),
  Blackout: command("Every channel goes out at zero. What is held stays held", "on", true),
  Undo: command("Take back the last change to the project"),

  // ---- tempo
  TapTempo: command("One tap. After two taps the beat follows the taps"),
  SetClockFeed: command("Which source drives the beat", "feed: tap, audio, midi clock, virtualdj, timecode"),
  SetSpeed: command("Sequences and patterns run at half, normal or double speed", "factor"),

  // ---- playing by hand
  StartLook: command("A look runs by hand, in the next frame", "look"),
  StopLook: command("A look that runs by hand is switched off. What it touched stays held until Release", "look"),
  EndFlash: command("A look that was flashed ends and gives back what it touched", "look"),
  SkipLook: command("One look of the show or the autopilot stops for the rest of the section", "look"),
  ReleaseHold: command("Give back one thing held by hand", "hold"),
  ReleaseAll: command("Give back everything held by hand", "", true),
  SetMaster: command("The level of everything", "level"),
  SetGroupLevel: command("The level of one fader: a group of lights or a rate", "fader, level"),
  SetPosition: command("Aim the movers. The position is held until Release", "pan, tilt"),
  GoCue: command("The cue list steps forward: its look starts, the one before stops", "list"),
  BackCue: command("The cue list steps back", "list"),
  Surprise: command("Start a look picked at random"),

  // ---- the Console layout
  AddPage: command("A new, empty Console page"),
  AddWidget: command("A widget on a page, at the first free place", "page, kind"),
  PlaceWidget: command("Move or resize a widget, in whole cells", "page, widget, x, y, w, h"),
  RemoveWidget: command("Take a widget off a page", "page, widget"),
  PutOnConsole: command("A look gets the next free pad of the Main page", "look"),

  // ---- songs
  SetFollows: command("Which deck or timecode gives the song and its position", "source: deck 1, deck 2, timecode"),
  PlayFrom: command("Run the show of a song from a section, without the deck", "song, section"),
  StopListening: command("End a run started with PlayFrom"),
  MarkReviewed: command("The sections of a song were checked by a person", "song"),
  MoveSectionEdge: command("Move the edge between two sections", "song, edge, to"),
  SplitSection: command("One section becomes two", "song, section"),
  MergeSections: command("A section and the next one become one", "song, section"),
  RenameSection: command("Another name for a section", "song, section, name"),
  AddSectionLook: command("A look plays in a section of a song's show", "song, section, look"),
  RemoveSectionLook: command("A look leaves a section", "song, section, look"),
  FillEmptySections: command("The autopilot picks once for every empty section and the picks are written into the show", "song"),
  SetShowMuted: command("The show of a song plays, or is silent", "song, on"),
  SetRecordedMuted: command("The Recorded row of a song plays, or is silent", "song, on"),
  UndoLastPass: command("Remove the newest finished pass from the Recorded row", "song"),
  ClearRecorded: command("Empty the Recorded row", "song"),

  // ---- looks
  SetStepFixtures: command("Which groups of lights a step sets", "step, groups"),
  SetStepProperty: command("Set one property of a step, or leave it alone", "step, property, value or not set"),
  SetSequenceEntry: command("Change one item of a sequence: start, width, speed, loop, spread, transition", "sequence, entry, change"),
  SetPattern: command("Change a pattern: shape, measure, colours from, runs on", "pattern, change"),
  AddGroup: command("A new, empty group"),
  RenameGroup: command("Another name for a group, in the autopilot rules too", "group, name"),
  DeleteGroup: command("Remove a group. Its looks stay", "group"),
  AddToGroup: command("A look joins a group", "look, group"),
  RemoveFromGroup: command("A look leaves a group", "look, group"),

  // ---- autopilot settings
  AddStyle: command("A new autopilot style"),
  RenameStyle: command("Another name for a style", "style, name"),
  DeleteStyle: command("Remove a style. The last one stays", "style"),
  AddRule: command("A new rule in a style", "style"),
  SetRule: command("Change a rule: when, pick, fit", "style, rule, change"),
  RemoveRule: command("Remove a rule", "style, rule"),
  SetAutopilotLights: command("A group of lights the autopilot may use, or not", "group, on"),
  SetWildness: command("How often the autopilot changes its pick", "level"),
  SetNeverDark: command("What plays when nothing else does", "look or none"),

  // ---- patch
  AddFixture: command("A light joins the rig", "model, mode, universe, address"),
  SetAddress: command("Another address for a light", "fixture, universe, address"),
  ReorderFixture: command("Move a light inside its group: the order is left to right", "fixture, by"),
  Identify: command("One light flashes so it can be found in the room, or stops", "fixture or none"),
  AddPanel: command("A pixel panel, spread over as many universes as it needs", "name, columns, rows, wiring, pixel, order, start universe"),

  // ---- raw channels
  SetChannel: command("Hold one channel by hand at a value", "channel, value", true),
  ReleaseChannel: command("Give one channel back", "channel", true),
  ReleaseChannels: command("Give back every channel held by hand, and nothing else"),
  SetWhoWins: command("One setting of the who-wins rules", "rule, value"),

  // ---- in and out
  TestOutput: command("Every channel of one output at half for three seconds", "output"),
  StartLearn: command("Wait for a hardware control to map to this control", "control"),
  CancelLearn: command("Stop waiting"),
  UpdateMapping: command("Change a mapping: device, note or controller number, feedback", "mapping, change"),
  TestMapping: command("Show that a mapping is alive: the hardware control lights. Nothing fires", "mapping"),
  RemoveMapping: command("Remove a mapping", "mapping"),

  // ---- in the engine today, with no control in any mockup
  SetOutput: command("Set up an output: its kind, universes and rate", "output, kind, universes, rate", true),
  RemoveOutput: command("Remove an output", "output", true),
} as const satisfies Record<string, CommandSpec>;

export type CommandName = keyof typeof COMMANDS;
