import { For, Show } from "solid-js";
import { store } from "../fake/store";
import { Button, Meter, Segmented, Tabs } from "../kit";
import { review } from "../review/store";
import { screens } from "./registry";
import { go, route } from "./router";

function Status() {
  return (
    <div class="status" role="group" aria-label="Status">
      <Segmented
        label="Output"
        layer="live"
        value={store.state.output}
        onChange={(v) => store.setOutput(v)}
        options={[
          { value: "live", label: "Live" },
          { value: "preview", label: "Preview" },
        ]}
      />
      <div class="status-tempo" aria-label={`Tempo ${store.state.tempo} BPM, beat ${store.beat() + 1}`}>
        <b class="display">{store.state.tempo}</b>
        <span class="mute">BPM</span>
        <span class="beat-dots" aria-hidden="true">
          <For each={[0, 1, 2, 3]}>{(i) => <i classList={{ "is-lit": store.beat() === i }} />}</For>
        </span>
      </div>
      <div class="status-master">
        <span class="mute">Master</span>
        <b class="display">{store.state.master}%</b>
        <Meter value={store.state.master / 100} label="Master level" class="status-meter" />
      </div>
      <Button
        variant="danger"
        pressed={store.isDark()}
        onClick={() => store.setBlackout(!store.state.blackout)}
        aria-keyshortcuts="Space"
      >
        {store.isDark() ? "Blackout on" : "Blackout"}
      </Button>
      <Button
        disabled={!store.undoLabel()}
        onClick={() => store.undo()}
        aria-label={store.undoLabel() ? `Undo ${store.undoLabel()}` : "Nothing to undo"}
        aria-keyshortcuts="Meta+Z Control+Z"
      >
        Undo
      </Button>
    </div>
  );
}

export function TopBar() {
  const tabs = () => [
    ...screens.map((s) => ({ id: s.id, label: s.title })),
    { id: "review", label: "Review" },
  ];

  return (
    <header class="topbar-wrap">
      <div class="topbar">
        <div class="brand">
          <b class="display">Bossylights</b>
          <span class="mute">{store.state.showName}</span>
        </div>
        <Status />
        <Show
          when={review.commentMode()}
          fallback={
            <Button
              class="review-toggle"
              pressed={review.panelOpen()}
              disabled={route() === "review"}
              onClick={() => review.setPanelOpen(!review.panelOpen())}
              aria-controls="review-panel"
            >
              Review
            </Button>
          }
        >
          <div class="comment-bar" role="status">
            <span>Comment mode: pick a region</span>
            <Button variant="primary" onClick={() => (review.setCommentMode(false), review.setActiveRegion(null))}>
              Done
            </Button>
          </div>
        </Show>
      </div>
      <nav class="tabbar" aria-label="Workspaces">
        <Tabs variant="workspace" label="Workspaces" items={tabs()} value={route()} onChange={go} />
      </nav>
    </header>
  );
}
