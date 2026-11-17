import { render } from "solid-js/web";
import "./styles/tokens.css";
import "./styles/base.css";
import "./kit/kit.css";
import "./shell/shell.css";
import "./review/review.css";
import { App } from "./App";
import { installShortcuts } from "./shell/shortcuts";

installShortcuts();
render(() => <App />, document.getElementById("root")!);
