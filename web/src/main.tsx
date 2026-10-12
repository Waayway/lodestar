import { render } from "preact";
import "@/styles/base.css";
import { App } from "@/App";

// A stored light/dark choice is applied by the inline script in index.html before the CSS
// paints; ThemeProvider (lib/theme) keeps <html data-theme> in step after that.
render(<App />, document.getElementById("app")!);
