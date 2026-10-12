// Light / dark: follows prefers-color-scheme unless the viewer picked one in the header.
// The pick is stored in localStorage when the browser allows it, and applied as
// <html data-theme="light|dark"> (tokens.css reads it). Without storage it lasts the visit.
import { createContext } from "preact";
import { useContext, useEffect, useState } from "preact/hooks";

export type ThemeChoice = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

export const THEME_KEY = "lodestar.theme";
export const THEME_CHOICES: readonly ThemeChoice[] = ["system", "light", "dark"];

function isChoice(v: unknown): v is ThemeChoice {
  return v === "system" || v === "light" || v === "dark";
}

export function readTheme(storage: Pick<Storage, "getItem"> | undefined = safeStorage()): ThemeChoice {
  try {
    const v = storage?.getItem(THEME_KEY);
    return isChoice(v) ? v : "system";
  } catch {
    return "system";
  }
}

export function writeTheme(
  choice: ThemeChoice,
  storage: Pick<Storage, "setItem" | "removeItem"> | undefined = safeStorage(),
): void {
  try {
    if (choice === "system") storage?.removeItem(THEME_KEY);
    else storage?.setItem(THEME_KEY, choice);
  } catch {
    // Blocked storage: the choice lasts until the page is reloaded.
  }
}

/** Sets or clears `data-theme` on the root element. */
export function applyTheme(choice: ThemeChoice, root: HTMLElement = document.documentElement): void {
  if (choice === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", choice);
}

function safeStorage(): Storage | undefined {
  try {
    return typeof localStorage === "undefined" ? undefined : localStorage;
  } catch {
    return undefined;
  }
}

const darkQuery = () =>
  typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: dark)") : null;

export type ThemeState = {
  choice: ThemeChoice;
  /** What is shown: the choice, or the system's preference for "system". */
  resolved: ResolvedTheme;
  setChoice: (c: ThemeChoice) => void;
};

export const ThemeContext = createContext<ThemeState>({
  choice: "system",
  resolved: "light",
  setChoice: () => {},
});

/** Theme state for `ThemeProvider`: persists and applies the choice, tracks the system. */
export function useThemeState(): ThemeState {
  const [choice, setChoiceState] = useState<ThemeChoice>(() => readTheme());
  const [systemDark, setSystemDark] = useState(() => darkQuery()?.matches ?? false);
  useEffect(() => {
    const q = darkQuery();
    if (!q) return;
    const on = (e: MediaQueryListEvent) => setSystemDark(e.matches);
    q.addEventListener("change", on);
    return () => q.removeEventListener("change", on);
  }, []);
  useEffect(() => applyTheme(choice), [choice]);
  const setChoice = (c: ThemeChoice) => {
    writeTheme(c);
    setChoiceState(c);
  };
  const resolved: ResolvedTheme = choice === "system" ? (systemDark ? "dark" : "light") : choice;
  return { choice, resolved, setChoice };
}

export function useTheme(): ThemeState {
  return useContext(ThemeContext);
}
