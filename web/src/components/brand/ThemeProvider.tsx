import type { ComponentChildren } from "preact";
import { ThemeContext, useThemeState } from "@/lib/theme";

/** Provides the theme choice (system / light / dark) and keeps <html data-theme> in step. */
export function ThemeProvider({ children }: { children: ComponentChildren }) {
  const state = useThemeState();
  return <ThemeContext.Provider value={state}>{children}</ThemeContext.Provider>;
}
