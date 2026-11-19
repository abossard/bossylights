// Generated from crates/engine-core. Do not edit.
// Update: UPDATE_TS=1 cargo test -p engine-core

export type Request = { "Send": { command: Command, } } | "Query" | "Subscribe" | "Stop" | { "Search": { text: string, } } | { "Fixture": { maker: string, model: string, } } | "Failures";

export type Command = { "SetChannel": { channel: ChannelNumbers, value: number, } } | { "ReleaseChannel": { channel: ChannelNumbers, } } | "ReleaseAll" | { "Blackout": { on: boolean, } } | { "SetOutput": { output: OutputNumber, kind: OutputKind, universes: RunNumbers, rate: Rate | null, } } | { "RemoveOutput": { output: OutputNumber, } } | { "Patch": { name: string, maker: string, model: string, mode: string, at: ChannelNumbers, } } | { "PatchStrip": { name: string, pixels: Pixels, color: Color, at: ChannelNumbers, } } | { "Move": { fixture: number, at: ChannelNumbers, } } | { "Unpatch": { fixture: number, } } | { "Rename": { fixture: number, name: string, } } | { "SetIntensity": { target: Target, intensity: number, } } | { "SetColour": { target: Target, red: number, green: number, blue: number, } } | { "ReleaseIntensity": { target: Target, } } | { "ReleaseColour": { target: Target, } } | { "SetGroup": { name: string, members: Array<Member>, } } | { "RemoveGroup": { name: string, } };

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

export type Reply = "Accepted" | { "Patched": { fixture: number, } } | "Stopping" | { "Refused": { reason: Refusal, } } | { "Picture": { state: State, project: Project, library: Library, 
/**
 * Every patched fixture, by number.
 */
patch: Array<Patched>, 
/**
 * Every group, in the order they were made.
 */
groups: Array<Grouped>, 
/**
 * Whether the patch and the groups are kept in a show file.
 */
show: ShowFile, universes: Array<Levels>, } } | { "Changed": { state: State, project: Project, library: Library, 
/**
 * The whole patch when it changed since the last message, else `None`.
 */
patch: Array<Patched> | null, 
/**
 * Every group when the patch changed since the last message, else `None`.
 */
groups: Array<Grouped> | null, 
/**
 * Whether the patch and the groups are kept in a show file.
 */
show: ShowFile, channels: Array<ChannelLevel>, } } | { "Found": { fixtures: Array<Match>, } } | { "Fixture": { maker: string, model: string, kind: FixtureKind, modes: Array<ModeChannels>, } } | { "Failures": { failures: Array<Failure>, } };

export type Refusal = "NotACommand" | { "NoSuchUniverse": { universes: number, } } | "NoSuchAddress" | "QueueFull" | "CannotOpen" | "NoSuchFixture" | "Opening" | { "TooManyUniverses": { most: number, } } | "NoSuchMode" | { "PastTheEnd": { channels: number, } } | { "NotStreamed": { first: number, last: number, } } | { "StreamsFixture": { fixture: number, } } | "NoLightsLeft" | "NotPatched" | "NoSuchLight" | "NoSuchGroup" | { "TooManyMembers": { most: number, } } | { "ShowFileFull": { most: number, } };

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
outputs: Array<Output | null>, };

export type Priority = "RealTime" | { "BestEffort": { reason: BestEffort, } };

export type BestEffort = { "Refused": { code: number, } } | "NotSupported";

export type Levels = number[];

export type ChannelLevel = { universe: number, address: number, value: number, };

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
layout: Layout | null, 
/**
 * The fixtures that share at least one of its channels. Allowed: the patch shows it.
 */
clashes: Array<number>, };

export type Pixels = number;

export type Layout = { width: number, height: number, };

export type Member = { "Light": { light: number, } } | { "Fixture": { fixture: number, } };

export type Grouped = { name: string, 
/**
 * In the order they were set.
 */
members: Array<Member>, };

export type ShowFile = "None" | "Kept" | "NotSaved" | { "LeftAlone": { why: Unopened, } };

export type Unopened = "CannotRead" | { "Broken": { line: number, column: number, } } | { "Newer": { version: number, } } | { "Taken": { fixture: number, } } | { "Refused": { reason: Refusal, } } | { "TooBig": { most: number, } };

export const API_VERSION = 2;
