// Generated from crates/engine-core. Do not edit.
// Update: UPDATE_TS=1 cargo test -p engine-core

export type Request = { "Send": { command: Command, } } | "Query" | "Subscribe" | "Stop" | { "Search": { text: string, } } | { "Fixture": { maker: string, model: string, } } | "Failures";

export type Command = { "SetChannel": { channel: ChannelNumbers, value: number, } } | { "ReleaseChannel": { channel: ChannelNumbers, } } | "ReleaseChannels" | "ReleaseAll" | { "Blackout": { on: boolean, } } | { "SetOutput": { output: OutputNumber, kind: OutputKind, universes: RunNumbers, rate: Rate | null, } } | { "RemoveOutput": { output: OutputNumber, } } | { "Patch": { name: string, maker: string, model: string, mode: string, at: ChannelNumbers, } } | { "PatchStrip": { name: string, pixels: Pixels, color: Color, at: ChannelNumbers, } } | { "Move": { fixture: number, at: ChannelNumbers, } } | { "Unpatch": { fixture: number, } } | { "Rename": { fixture: number, name: string, } } | { "SetIntensity": { target: Target, intensity: number, } } | { "SetColour": { target: Target, red: number, green: number, blue: number, } } | { "ReleaseIntensity": { target: Target, } } | { "ReleaseColour": { target: Target, } } | { "SetProperty": { target: Target, value: Value, } } | { "UpdateGrab": { grab: GrabId, value: Value, } } | { "EndGrab": { grab: GrabId, } } | { "ReleaseProperty": { target: Target, property: Property, } } | { "SetGroup": { name: string, members: Array<Member>, } } | { "RemoveGroup": { name: string, } } | { "PutStep": { look: number, name: string, step: AuthoredStep, } } | { "PutPattern": { look: number, name: string, pattern: Pattern, } } | { "PutSequence": { look: number, name: string, sequence: Sequence, } } | { "RemoveLook": { look: number, } } | { "StartLook": { look: number, use?: Use, rank?: Rank, 
/**
 * At once when not given, or on the next beat or bar of the grid: then the
 * play waits, and plays from the frame of that crossing.
 */
when?: When, } } | { "StopLook": { play: PlayId, } } | { "EndFlash": { play: PlayId, } } | { "ReleaseHold": { play: PlayId, } } | { "SetMixRules": { rules: MixRules, } } | { "SetInternalTempo": { bpm: Bpm, } } | "TapTempo" | "ReleaseTempo";

export type ChannelNumbers = { universe: number, address: number, };

export type RunNumbers = { first: number, count: number, };

export type OutputNumber = number;

export type OutputKind = { "Ddp": { address: string, 
/**
 * Whole pixels only: RGB sends 510 of a universe's 512 channels, RGBW all 512.
 */
color: Color, } } | { "DmxUsbPro": { adapter: Adapter, } };

export type Adapter = number;

export type Color = "Rgb" | "Rgbw";

export type Rate = "30" | "44" | "60";

export type Reply = "Accepted" | { "Patched": { fixture: number, } } | { "Started": { play: PlayId, } } | { "Grabbed": { grab: GrabId, } } | "Stopping" | { "Refused": { reason: Refusal, } } | { "Picture": { state: State, project: Project, library: Library, 
/**
 * Every patched fixture, by number.
 */
patch: Array<Patched>, 
/**
 * Every group, in the order they were made.
 */
groups: Array<Grouped>, 
/**
 * Every saved look, by number.
 */
looks: Array<SavedLook>, 
/**
 * Whether the patch, the groups and the looks are kept in a show file.
 */
show: ShowFile, universes: Array<Levels>, 
/**
 * Every light that holds a part, by number.
 */
lights: Array<HeldLight>, health: Health, } } | { "Health": { health: Health, events: Array<OutputHealthEvent>, missed: number, } } | { "Changed": { state: State, project: Project, library: Library, 
/**
 * The whole patch when it changed since the last message, else `None`.
 */
patch: Array<Patched> | null, 
/**
 * Every group when the patch changed since the last message, else `None`.
 */
groups: Array<Grouped> | null, 
/**
 * Every saved look when they changed since the last message, else `None`.
 */
looks: Array<SavedLook> | null, 
/**
 * Whether the patch, the groups and the looks are kept in a show file.
 */
show: ShowFile, channels: Array<ChannelLevel>, 
/**
 * Every light whose held parts changed, as it holds them now: a part let go is
 * `None`.
 */
lights: Array<HeldLight>, } } | { "Found": { fixtures: Array<Match>, } } | { "Fixture": { maker: string, model: string, kind: FixtureKind, modes: Array<ModeChannels>, } } | { "Failures": { failures: Array<Failure>, } };

export type Health = { window_ms: number, outputs: Array<OutputHealth>, };

export type OutputHealth = { output: OutputNumber, generation: number, status: Status, state: OutputHealthState, measurement: OutputMeasurement, };

export type OutputHealthState = "Measuring" | "Healthy" | { "Late": { reason: LateReason, } } | { "Recovering": { reason: LateReason, } };

export type OutputMeasurement = "Measuring" | "ClockDiscontinuity" | { "Window": { end_ms: number, lateness: LatenessWindow | null, press_to_light: PressWindow | null, press_missing: number, dropped: number, failed: number, off_beat: number, frames_handed: number, } };

export type LatenessWindow = { p99_us: number, worst_us: number, samples: number, };

export type PressWindow = { p95_us: number, worst_us: number, samples: number, };

export type LateReason = "HandoffLateness" | "PressToLight" | "Both";

export type RecoveredReason = "WithinTargets" | "OutputAvailable";

export type OutputHealthEvent = { output: OutputNumber, generation: number, at_ms: number, event: HealthEvent, };

export type HealthEvent = { "RunningLate": { reason: LateReason, } } | { "Recovered": { reason: RecoveredReason, } } | "OutputLost";

export type Refusal = "NotACommand" | { "NoSuchUniverse": { universes: number, } } | "NoSuchAddress" | "QueueFull" | { "CannotOpen": { why: OpenFailure, } } | "NoSuchFixture" | "Opening" | { "TooManyUniverses": { most: number, } } | "NoSuchMode" | { "PastTheEnd": { channels: number, } } | { "NotStreamed": { first: number, last: number, } } | { "NotSent": { output: number, universe: number, address: number, } } | { "StreamsFixture": { fixture: number, } } | { "Overlaps": { fixture: number, } } | "NoLightsLeft" | "NotPatched" | "NoSuchLight" | "NoSuchGroup" | { "TooManyMembers": { most: number, } } | { "ShowFileFull": { most: number, } } | "NoSuchLook" | "WrongLookKind" | "LookCycle" | "InvalidTiming" | "EmptyPalette" | "NoMatrix" | "WorkLimit" | { "LookInUse": { look: number, } } | { "TooManyLooks": { most: number, } } | "PlayFull" | { "Unsupported": { light: number, property: Property, } } | "Stale" | "EmptyGroup" | { "OtherProperty": { property: Property, } } | { "GrabFull": { most: number, } };

export type OpenFailure = { "Device": { with: number | null, } } | "Thread" | "Unavailable";

export type State = { 
/**
 * Frames computed since the engine started.
 */
frame: number, 
/**
 * What the system gave the output thread. Asked once, when the thread starts.
 */
priority: Priority, 
/**
 * While on, every channel goes out at zero. The held levels stay.
 */
blackout: boolean, 
/**
 * The outputs, by number. `None` where no output is set.
 */
outputs: Array<Output | null>, 
/**
 * The looks playing or kept, by slot.
 */
plays: Array<Play>, 
/**
 * The Mixer's settings as this frame applies them.
 */
rules: MixRules, 
/**
 * The beat clock, settled to the frame's time.
 */
clock: Reading, };

export type Priority = "RealTime" | { "BestEffort": { reason: BestEffort, } };

export type BestEffort = { "Refused": { code: number, } } | "NotSupported";

export type Levels = number[];

export type ChannelLevel = { universe: number, address: number, value: number, };

export type HeldLight = { light: number, intensity: number | null, colour: Rgb | null, };

export type Rgb = { red: number, green: number, blue: number, };

export type Light = { intensity: number | null, colour: Rgb | null, position: Position | null, zoom: number | null, 
/**
 * The slot of each gobo wheel, wheel 1 first.
 */
gobos: [number | null, number | null, number | null], };

export type Position = { pan: number, tilt: number, };

export type Gobo = { wheel: Wheel, slot: number, };

export type Wheel = number;

export type Property = "Intensity" | "Colour" | "Position" | "Zoom" | { "Gobo": Wheel };

export type Value = { "Intensity": number } | { "Colour": Rgb } | { "Position": Position } | { "Zoom": number } | { "Gobo": Gobo };

export type GrabId = { generation: number, };

export type Step = { target: Target, values: Light, };

export type AuthoredStep = { target: Target, values: Light, swatches?: Swatches, };

export type Swatches = Array<Rgb>;

export type Pattern = { "Wave": { target: WaveTarget, period: number, palette: Palette, } };

export type WaveTarget = { "Group": { group: string, } } | { "Matrix": { fixture: number, } };

export type Palette = { "Inline": Swatches } | { "Step": { look: number, } };

export type Saved = { look: number, name: string, step: Step, };

export type SavedLook = { look: number, name: string, step: AuthoredStep } | { look: number, name: string, sequence: Sequence } | { look: number, name: string, pattern: Pattern };

export type Sequence = { items: Array<SequenceItem>, };

export type SequenceItem = { look: number, width?: number, transition?: number, use?: Use, };

export type Use = { speed?: Speed, repeat?: Repeat, spread?: number, };

export type Speed = "Half" | "Normal" | "Double";

export type Repeat = "Loop" | "Once";

export type PlayId = { slot: number, generation: number, };

export type Play = { play: PlayId, look: number, rank: Rank, state: PlayState, 
/**
 * In beats, as the picture shows it.
 */
start: number, };

export type Rank = "Autopilot" | "Show" | "Manual";

export type PlayState = { "Waiting": { beat: number } } | "Playing" | "Kept";

export type When = "Now" | "Beat" | "Bar";

export type Bpm = number;

export type Reading = { 
/**
 * The tempo in charge.
 */
bpm: number, 
/**
 * The internal feed's tempo.
 */
feed: Bpm, source: "Feed" | "OneTap" | "Tapped", 
/**
 * Where the grid is, in beats: a whole number is a beat, a multiple of four a bar.
 */
beat: number, 
/**
 * Beats elapsed since the engine started. A play's own beat is this less its start.
 */
elapsed: number, };

export type MixRules = { brightness: Brightness, manual_end: ManualEnd, other_end: OtherEnd, };

export type Brightness = "Latest" | "Highest";

export type ManualEnd = "UntilRelease" | "UntilStop";

export type OtherEnd = "Reveal" | "Stay";

export type Output = { universes: Run, rate: Rate, status: Status, counters: Counters, };

export type Run = { 
/**
 * Counted from 1, as the client sent it.
 */
first: number, count: number, };

export type Status = "NotFound" | "Connecting" | "Running" | { "Reconnecting": { failures: number, } };

export type Counters = { accepted: number, dropped: number, failed: number, 
/**
 * Intervals more than [`OFF_BEAT`] away from the period.
 */
off_beat: number, };

export type Library = "NoFolder" | "Reading" | { "Read": { read: number, failed: number, } } | "CannotOpen";

export type Match = { maker: string, model: string, kind: FixtureKind, modes: Array<ModeSize>, };

export type ModeSize = { name: string, channels: number, };

export type ModeChannels = { name: string, channels: Array<NamedChannel>, };

export type NamedChannel = { name: string, function: ChannelFunction, };

export type Failure = { file: string, line: number | null, problem: string, };

export type FixtureKind = "ColorChanger" | "Dimmer" | "Effect" | "Fan" | "Flower" | "Hazer" | "Laser" | "LedBarBeams" | "LedBarPixels" | "MovingHead" | "Other" | "Scanner" | "Smoke" | "Strobe" | { "Unlisted": { name: string, } };

export type ChannelFunction = "Dimmer" | "Red" | "Green" | "Blue" | "White" | "Amber" | "Uv" | "Indigo" | "Lime" | "Cyan" | "Magenta" | "Yellow" | "Hue" | "Saturation" | "Lightness" | "Value" | "ColorTemperature" | "Pan" | "Tilt" | "Zoom" | "Focus" | "Iris" | "Frost" | "Gobo" | "ColorWheel" | "Strobe" | "Shutter" | "Prism" | "Speed" | "Effect" | "Control" | "NoFunction" | { "Other": { name: string, } };

export type Project = { 
/**
 * The outputs, by number. `None` where none is set.
 */
outputs: Array<ProjectOutput | null>, };

export type ProjectOutput = { "Kind": { kind: OutputKind, universes: Run, } } | { "Attached": { universes: Run, } };

export type Target = { "Light": { light: number, } } | { "Fixture": { fixture: number, } } | { "Group": { group: string, } };

export type Patched = { 
/**
 * Its number, from 1. Kept while it is patched.
 */
fixture: number, name: string, maker: string, model: string, mode: string, 
/**
 * Its first channel.
 */
universe: number, address: number, 
/**
 * How many channels it takes from its address on.
 */
channels: number, 
/**
 * Its lights, numbered from `first` (counted from 1).
 */
first: number, lights: number, 
/**
 * The grid of its heads, `None` for a fixture without heads or a definition that
 * gives none that fits. Its heads are its last lights, row by row.
 */
layout: Layout | null, };

export type Pixels = number;

export type Layout = { width: number, height: number, };

export type Member = { "Light": { light: number, } } | { "Fixture": { fixture: number, } };

export type Grouped = { name: string, 
/**
 * In the order they were set.
 */
members: Array<Member>, };

export type ShowFile = "None" | "Kept" | "NotSaved" | { "LeftAlone": { why: Unopened, } };

export type Unopened = "CannotRead" | { "Broken": { line: number, column: number, } } | { "Newer": { version: number, } } | { "Older": { version: number, } } | { "Taken": { fixture: number, } } | { "LookTaken": { look: number, } } | { "Refused": { reason: Refusal, } } | { "TooBig": { most: number, } };

export const API_VERSION = 11;
