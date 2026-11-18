/**
 * | state    | means                          |
 * |----------|--------------------------------|
 * | plain    | a channel at rest              |
 * | selected | its fader shows at the right   |
 * | held     | held by hand: pink             |
 * | changing | its value is changing now      |
 * | clash    | two lights share it: red       |
 */
export type CellState = "plain" | "selected" | "held" | "changing" | "clash";

export interface CellProps {
  /** 1 to 512. */
  address: number;
  /** What uses it: "PAR 3 red". Empty when no light does. */
  name: string;
  /** 0 to 255. */
  value: number;
  state?: CellState;
  onPress: () => void;
}

/** One channel of a universe: address, who uses it, its value now. */
export function Cell(props: CellProps) {
  return (
    <button
      type="button"
      class="k-cell"
      data-state={props.state ?? "plain"}
      aria-current={props.state === "selected"}
      aria-label={`Channel ${props.address}, ${props.name || "not used"}, at ${props.value}`}
      onClick={() => props.onPress()}
    >
      <span class="k-cell-address">{props.address}</span>
      <span class="k-cell-value">{props.value}</span>
      <span class="k-cell-name">{props.name}</span>
    </button>
  );
}
