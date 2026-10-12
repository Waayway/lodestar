import { render } from "preact";
import "@/styles/base.css";
import { App } from "@/App";
import { applyTheme, readTheme } from "@/lib/theme";

// Before the first paint, so a stored light/dark choice doesn't flash the system theme.
applyTheme(readTheme());
render(<App />, document.getElementById("app")!);
