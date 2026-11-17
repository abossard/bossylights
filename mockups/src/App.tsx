import { Dynamic } from "solid-js/web";
import { Match, Show, Switch } from "solid-js";
import { store } from "./fake/store";
import { NowStrip, stepWindow } from "./kit";
import { ReviewPanel } from "./review/ReviewPanel";
import { ReviewScreen } from "./review/ReviewScreen";
import { ScreenContext } from "./review/store";
import { screenById, screens } from "./shell/registry";
import { go, route } from "./shell/router";
import { TopBar } from "./shell/TopBar";

export function App() {
  const current = () => (route() === "review" ? null : screenById(route()) ?? screens[0]);
  const strip = () => {
    const s = current();
    return s ? s.nowStrip !== false : true;
  };
  const stripWindow = () => stepWindow(store.now(), 5 * 60, 15 * 60, 60);

  return (
    <div class="app" classList={{ "is-dark": store.isDark() }}>
      <TopBar />
      <Show when={strip()}>
        <NowStrip
          from={stripWindow().from}
          to={stripWindow().to}
          now={store.now()}
          lanes={store.lanes(stripWindow().from, stripWindow().to)}
          onActivate={() => go("console")}
        />
      </Show>
      <main class="screen-host" id="main">
        <Switch>
          <Match when={route() === "review"}>
            <ReviewScreen />
          </Match>
          <Match when={current()} keyed>
            {(s) => (
              <ScreenContext.Provider value={s.id}>
                <Dynamic component={s.Screen} />
              </ScreenContext.Provider>
            )}
          </Match>
        </Switch>
      </main>
      <ReviewPanel />
    </div>
  );
}
