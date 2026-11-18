import { Button } from "./Button.tsx";

export interface StepperProps {
  label: string;
  /** The value as it reads: "1 beat", "A 17". */
  value: string;
  /** The words on the two buttons. */
  less: string;
  more: string;
  /** Why a side cannot go further. */
  lessOff?: string;
  moreOff?: string;
  onStep: (by: -1 | 1) => void;
}

/** A value between two buttons. A side that cannot go further says why. */
export function Stepper(props: StepperProps) {
  return (
    <div class="k-stepper" role="group" aria-label={props.label}>
      <Button label={props.less} {...(props.lessOff === undefined ? {} : { off: props.lessOff })} onPress={() => props.onStep(-1)} />
      <span class="k-stepper-value">
        <span class="k-field-label">{props.label}</span>
        {props.value}
      </span>
      <Button label={props.more} {...(props.moreOff === undefined ? {} : { off: props.moreOff })} onPress={() => props.onStep(1)} />
    </div>
  );
}
