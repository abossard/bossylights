// Generated from crates/engine-core. Do not edit.
// Update: UPDATE_TS=1 cargo test -p engine-core

export type Request = { "Send": { command: Command, } } | "Query" | "Subscribe";

export type Command = { "SetChannel": { channel: ChannelNumbers, value: number, } } | { "ReleaseChannel": { channel: ChannelNumbers, } } | "ReleaseAll" | { "Blackout": { on: boolean, } } | { "SetOutput": { output: OutputNumber, kind: OutputKind, universes: RunNumbers, rate: Rate | null, } } | { "RemoveOutput": { output: OutputNumber, } };

export type ChannelNumbers = { universe: number, address: number, };

export type RunNumbers = { first: number, count: number, };

export type OutputNumber = number;

export type OutputKind = { "Ddp": { address: string, color: Color, } } | "Mock";

export type Color = "Rgb" | "Rgbw";

export type Rate = "30" | "44" | "60";

export type Reply = "Accepted" | { "Refused": { reason: Refusal, } } | { "Picture": { state: State, universes: Array<Levels>, } } | { "Changed": { state: State, channels: Array<ChannelLevel>, } };

export type Refusal = "NotACommand" | { "NoSuchUniverse": { universes: number, } } | "NoSuchAddress" | "QueueFull" | "CannotOpen";

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

export type Output = { kind: OutputKind, universes: Run, rate: Rate, status: Status, counters: Counters, };

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
