// Generated from crates/engine-core. Do not edit.
// Update: UPDATE_TS=1 cargo test -p engine-core

export type Request = { "Send": { command: Command, } } | "Query" | "Subscribe";

export type Command = { "SetChannel": { channel: ChannelNumbers, value: number, } } | { "ReleaseChannel": { channel: ChannelNumbers, } } | "ReleaseAll" | { "Blackout": { on: boolean, } };

export type ChannelNumbers = { universe: number, address: number, };

export type Reply = "Accepted" | { "Refused": { reason: Refusal, } } | { "Picture": { state: State, universes: Array<Levels>, } } | { "Changed": { state: State, channels: Array<ChannelLevel>, } };

export type Refusal = "NotACommand" | { "NoSuchUniverse": { universes: number, } } | "NoSuchAddress" | "QueueFull";

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
blackout: boolean, };

export type Priority = "RealTime" | { "BestEffort": { reason: BestEffort, } };

export type BestEffort = { "Refused": { code: number, } } | "NotSupported";

export type Levels = number[];

export type ChannelLevel = { universe: number, address: number, value: number, };
