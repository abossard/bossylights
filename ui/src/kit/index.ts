// The kit: ten primitives, named by what they are for (issue #102). Every control of the behaviour table
// is one of them with props, or a few of them together. Each is a view: props in, a gesture out. None imports the engine.
//
// | primitive | is for                                    | kinds                                   |
// |-----------|-------------------------------------------|-----------------------------------------|
// | Button    | do it, or switch it                       | action, toggle                          |
// | Pad       | play a look                               |                                         |
// | Fader     | set one value                             |                                         |
// | XY        | set two values at once                    |                                         |
// | Picker    | choose one of a few                       | tabs, segments, cards, list             |
// | Field     | give a value in words or in steps         | text, search, steps                     |
// | Status    | read a state. Never pressed               | lamp, pill, flag, value                 |
// | Item      | a thing where it lives: pick it, open it  | row, tile, cell, entry, chip, strip     |
// | Block     | a thing placed in time or on a grid       | bar, frame                              |
// | Handle    | the edge of a thing: only dragged         | edge, corner                            |
import "./kit.css";

export { Block, type BlockProps } from "./Block.tsx";
export { Button, type ButtonProps, type ToggleMark, type ToggleState } from "./Button.tsx";
export { Fader } from "./Fader.tsx";
export { Field, type FieldProps } from "./Field.tsx";
export { Handle } from "./Handle.tsx";
export { Item, type ItemKind, type ItemProps } from "./Item.tsx";
export { Pad, type PadState } from "./Pad.tsx";
export { named, Picker, type PickerKind, type PickOption } from "./Picker.tsx";
export { Status, type StatusFill, type StatusKind, type StatusProps } from "./Status.tsx";
export { XY } from "./XY.tsx";
export { COLOURS, offWith, type Colour, type Pressable, type Signal, type Standing } from "./common.ts";
