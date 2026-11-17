import { For, Show, type JSX } from "solid-js";

export interface Column<R> {
  key: string;
  title: string;
  align?: "left" | "right" | "center";
  /** CSS width, e.g. "120px" or "20%". */
  width?: string;
  render?: (row: R) => JSX.Element;
}

export interface TableProps<R> {
  columns: Column<R>[];
  rows: R[];
  rowKey: (row: R) => string;
  /** Makes rows clickable (the first cell becomes a button). */
  onRowClick?: (row: R) => void;
  selected?: string | null;
  /** Ruled and tighter, for expert screens. */
  dense?: boolean;
  caption?: string;
  empty?: JSX.Element;
  class?: string;
}

/** A plain table. Numbers are tabular. */
export function Table<R>(props: TableProps<R>) {
  const cell = (row: R, c: Column<R>) =>
    c.render ? c.render(row) : String((row as Record<string, unknown>)[c.key] ?? "");

  return (
    <table class={`table ${props.dense ? "table-dense" : ""} ${props.class ?? ""}`}>
      <Show when={props.caption}>
        <caption class="visually-hidden">{props.caption}</caption>
      </Show>
      <thead>
        <tr>
          <For each={props.columns}>
            {(c) => (
              <th scope="col" style={{ width: c.width, "text-align": c.align ?? "left" }}>
                {c.title}
              </th>
            )}
          </For>
        </tr>
      </thead>
      <tbody>
        <For each={props.rows} fallback={<tr><td colSpan={props.columns.length} class="table-empty">{props.empty}</td></tr>}>
          {(row) => (
            <tr classList={{ "is-selected": props.selected === props.rowKey(row), "is-clickable": !!props.onRowClick }}>
              <For each={props.columns}>
                {(c, i) => (
                  <td style={{ "text-align": c.align ?? "left" }}>
                    <Show when={i() === 0 && props.onRowClick} fallback={cell(row, c)}>
                      <button type="button" class="table-rowbtn" onClick={() => props.onRowClick?.(row)}>
                        {cell(row, c)}
                      </button>
                    </Show>
                  </td>
                )}
              </For>
            </tr>
          )}
        </For>
      </tbody>
    </table>
  );
}
