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
  SetLock: command("Lock what a press does while LIVE is on, or not: the Console layout, pads, pages, Learn and the mappings", "on"),
  Blackout: command("Every channel goes out at zero. What is held stays held", "on", true),
  Undo: command("Take back the last change to the show file"),
  Redo: command("Put back the last change Undo took away"),

  // ---- the show file
  RenameShow: command("Another name for the show and its file", "name"),
  NewShow: command("An empty show file. The open one is saved first"),
  OpenShow: command("Open another show file. The open one is saved first", "file"),
  SaveShowAs: command("Write a copy of the show file. The copy becomes the open one", "file"),

  // ---- tempo
  TapTempo: command("One tap. After two taps the beat follows the taps, until ReleaseTempo"),
  ReleaseTempo: command("The taps let go: the beat follows the picked feed again"),
  SetTempoFeed: command("Which source drives the beat", "feed: tap, audio, midi clock, virtualdj, timecode"),
  SetFollows: command("Which deck or timecode gives the song and its position", "source: deck 1, deck 2, timecode"),
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

  // ---- the Console layout
  AddPage: command("A new, empty Console page"),
  RenamePage: command("Another name for a page", "page, name"),
  MovePage: command("Move a page among the tabs", "page, by"),
  DeletePage: command("Remove a page and its widgets. The last page stays", "page"),
  SetWidgetTarget: command("What a widget acts on: the look of a pad, the target of a slider", "page, widget, target"),
  SetWidgetWords: command("The words of a label or a frame", "page, widget, words"),
  SetCueList: command("The looks of a cue list and their order", "list, looks"),
  AddWidget: command("A widget on a page, at the first free place", "page, kind"),
  PlaceWidget: command("Move or resize a widget, in whole cells", "page, widget, x, y, w, h"),
  RemoveWidget: command("Take a widget off a page", "page, widget"),
  PutOnConsole: command("A look gets the next free pad of the Main page", "look"),

  // ---- songs
  PlayFrom: command("Play the show of a song from a section, without the deck", "song, section"),
  StopPlayFrom: command("End what PlayFrom started"),
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
  AddLook: command("A new step, sequence or pattern", "kind"),
  RenameLook: command("Another name for a look, everywhere it is used", "look, name"),
  CopyLook: command("A copy of a look, in the same groups", "look"),
  DeleteLook: command("Remove a look from the library, its pads and its sections. Refused while a sequence uses it", "look"),
  AddSequenceEntry: command("One more item at the end of a sequence", "sequence, look"),
  RemoveSequenceEntry: command("Take one item out of a sequence", "sequence, entry"),
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
  AddFixture: command("A light joins the patch", "model, mode, universe, address"),
  SetAddress: command("Another address for a light", "fixture, universe, address"),
  ReorderFixture: command("Move a light inside its group: the order is left to right", "fixture, by"),
  Identify: command("One light flashes so it can be found in the room, or stops", "fixture or none"),
  RenameFixture: command("Another name for a light", "fixture, name"),
  SetFixtureMode: command("Another mode of the same model. Refused where its channels do not fit", "fixture, mode"),
  SetFixtureGroup: command("A light joins another group, as its last", "fixture, group"),
  RemoveFixture: command("A light leaves the patch", "fixture"),
  RemovePanel: command("A pixel panel leaves the patch", "panel"),
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

  SetOutput: command("Set up an output: its kind, universes and rate", "output, kind, universes, rate", true),
  RemoveOutput: command("Remove an output", "output", true),
  SetAudioInput: command("Which input the beat is heard on", "input"),
  SetOsc: command("Listen for commands over OSC, or not, and on which port", "on, port"),

  // ---- the engine itself
  StopEngine: command("The engine ends in an orderly way: it answers, closes every connection and sends no more frames. The request Stop of the API (#107)", "", true),

} as const satisfies Record<string, CommandSpec>;

export type CommandName = keyof typeof COMMANDS;
