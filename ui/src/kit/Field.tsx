import { createSignal, Show } from "solid-js";
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
  /** Why a text does not fit: a calculation, undefined when it fits. A refused text is never sent and the old value stays. */
  refuse?: (text: string) => string | undefined;
  /**
   * `enter or leave`: leaving the field saves too, as Enter does. Leaving with a text that is refused puts the old one back, either way.
   * `enter`: leaving keeps a text that fits as it is, not saved, and says so under the field. An empty one puts the old one back.
   */
  saves?: "enter" | "enter or leave";
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

/** What a field that saves only on Enter says when it is left with a text that is not saved. */
const NOT_SAVED = "Not saved. Enter saves it";

/**
 * A text field. Enter saves, Escape puts the old value back. An empty field keeps the old value.
 * A text that does not fit is refused: nothing is sent, the reason stands under the field, the old value stays.
 * Text on screen is the value or says it is not saved: it never differs from the value unsaid.
 */
function Text(props: TextProps) {
  // Why the last text was refused, and whether a text that fits was left unsaved: the field's own state, gone with the next letter.
  const [refused, setRefused] = createSignal<string>();
  const [unsaved, setUnsaved] = createSignal(false);
  const save = (input: HTMLInputElement) => {
    const text = input.value.trim();
    const reason = text === "" || text === props.value ? undefined : props.refuse?.(text);
    setRefused(reason);
    setUnsaved(false);
    if (text === "") input.value = props.value;
    else if (reason === undefined && text !== props.value) props.onText(text);
  };
  const key = (e: KeyboardEvent & { currentTarget: HTMLInputElement }) => {
    if (props.kind === "search") return;
    if (e.key === "Enter") save(e.currentTarget);
    if (e.key === "Escape") {
      e.currentTarget.value = props.value;
      setRefused(undefined);
      setUnsaved(false);
    }
  };
  // Leaving saves where it says so. Elsewhere a text that does not fit is refused on leaving too: it is never kept unsaid.
  const leave = (e: FocusEvent & { currentTarget: HTMLInputElement }) => {
    if (props.kind === "search") return;
    if (props.saves === "enter or leave") save(e.currentTarget);
    else {
      const text = e.currentTarget.value.trim();
      setRefused(text === "" || text === props.value ? undefined : props.refuse?.(text));
      setUnsaved(refused() === undefined && text !== "" && text !== props.value);
      if (text === "") e.currentTarget.value = props.value;
    }
    if (refused() !== undefined) e.currentTarget.value = props.value;
  };
  return (
    <label class="k-field" classList={{ "is-refused": refused() !== undefined }}>
      <span class="k-field-label">{props.label}</span>
      <input
        class="k-input"
        type={props.kind === "search" ? "search" : "text"}
        value={props.value}
        placeholder={props.placeholder ?? ""}
        disabled={props.off !== undefined}
        aria-invalid={refused() !== undefined}
        onInput={(e) => {
          if (props.kind === "search") props.onText(e.currentTarget.value);
          setRefused(undefined);
          setUnsaved(false);
        }}
        onKeyDown={key}
        onBlur={leave}
      />
      <Show when={props.off ?? refused()} fallback={<Show when={unsaved()}><span class="k-sub k-unsaved">{NOT_SAVED}</span></Show>}>
        {(reason) => <span class="k-reason">{reason()}</span>}
      </Show>
    </label>
  );
}
