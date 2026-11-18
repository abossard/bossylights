import { Show } from "solid-js";
import { Button } from "./Button.tsx";
import { offWith, type Pressable } from "./common.ts";
import { Status } from "./Status.tsx";

interface Shared extends Pressable {
  label: string;
  /** The value as it reads: the text, or "1 beat", "A 17". */
  value: string;
}

export interface TextProps extends Shared {
  /**
   * | kind   | sends                                                    |
   * |--------|----------------------------------------------------------|
   * | text   | on Enter. Escape puts the old text back                  |
   * | search | with every letter                                        |
   */
  kind: "text" | "search";
  placeholder?: string;
  onText: (text: string) => void;
}

export interface StepsProps extends Shared {
  /** A value between two buttons: one step less, one step more. */
  kind: "steps";
  /** The words on the two buttons. */
  less: string;
  more: string;
  /** Why a side cannot go further. */
  lessOff?: string;
  moreOff?: string;
  onStep: (by: -1 | 1) => void;
}

export type FieldProps = TextProps | StepsProps;

/** A value given in words or in steps, with its label above. 44 px high. */
export function Field(props: FieldProps) {
  return (
    <Show when={props.kind === "steps" ? props : undefined} fallback={<Text {...(props as TextProps)} />}>
      {(steps) => (
        <div class="k-steps" role="group" aria-label={steps().label}>
          <Button label={steps().less} {...offWith(steps().off ?? steps().lessOff)} onPress={() => steps().onStep(-1)} />
          <Status kind="value" name={steps().label} label={steps().value} />
          <Button label={steps().more} {...offWith(steps().off ?? steps().moreOff)} onPress={() => steps().onStep(1)} />
        </div>
      )}
    </Show>
  );
}

function Text(props: TextProps) {
  const key = (e: KeyboardEvent & { currentTarget: HTMLInputElement }) => {
    if (props.kind === "search") return;
    if (e.key === "Enter") props.onText(e.currentTarget.value);
    if (e.key === "Escape") e.currentTarget.value = props.value;
  };
  return (
    <label class="k-field">
      <span class="k-field-label">{props.label}</span>
      <input
        class="k-input"
        type={props.kind === "search" ? "search" : "text"}
        value={props.value}
        placeholder={props.placeholder ?? ""}
        disabled={props.off !== undefined}
        onInput={(e) => props.kind === "search" && props.onText(e.currentTarget.value)}
        onKeyDown={key}
      />
      <Show when={props.off}>{(reason) => <span class="k-reason">{reason()}</span>}</Show>
    </label>
  );
}
