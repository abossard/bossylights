import { For } from "solid-js";
import { Region } from "../kit";

export interface StubRegion {
  id: string;
  title: string;
  /** One line: what will be here. */
  line: string;
  /** grid-area name used in the layout. */
  area: string;
}

/**
 * A screen that is not built yet: its regions from the brief as empty titled
 * regions, so navigation and review already work on it.
 */
export function Stub(props: { regions: StubRegion[]; areas: string; columns: string; rows?: string }) {
  return (
    <div
      class="stub"
      style={{ "grid-template-areas": props.areas, "grid-template-columns": props.columns, "grid-template-rows": props.rows ?? "auto" }}
    >
      <For each={props.regions}>
        {(r) => (
          <Region id={r.id} title={r.title} style={{ "grid-area": r.area }}>
            <p class="stub-line">{r.line}</p>
          </Region>
        )}
      </For>
    </div>
  );
}
