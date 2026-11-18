export interface NodeProps {
  label: string;
  /** What it is, in one small word: Sequence, Console, Song, Autopilot. */
  kind: string;
  open?: boolean;
  onPress: () => void;
}

/** A box in a picture of how things connect. 44 px, outlined, with a small word for what it is. */
export function Node(props: NodeProps) {
  return (
    <button type="button" class="k-node" classList={{ "is-on": props.open === true }} aria-current={props.open === true} onClick={() => props.onPress()}>
      <span class="k-sub">{props.kind}</span>
      <span class="k-label">{props.label}</span>
    </button>
  );
}
