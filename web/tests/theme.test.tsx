// @vitest-environment jsdom
// Theme: the toggle sets <html data-theme> and remembers it; storage may be missing; the
// inline script in index.html reads the same key before the CSS paints.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, describe, expect, it } from "vitest";
import { ThemeProvider } from "@/components/brand/ThemeProvider";
import { ThemeToggle } from "@/components/brand/ThemeToggle";
import { readTheme, THEME_KEY, writeTheme } from "@/lib/theme";

afterEach(() => {
  cleanup();
  document.documentElement.removeAttribute("data-theme");
  localStorage.clear();
});

describe("ThemeToggle", () => {
  it("sets data-theme on <html> and remembers the choice; system clears both", () => {
    render(
      <ThemeProvider>
        <ThemeToggle />
      </ThemeProvider>,
    );
    const root = document.documentElement;
    fireEvent.click(screen.getByRole("button", { name: "Dark theme" }));
    expect(root.getAttribute("data-theme")).toBe("dark");
    expect(localStorage.getItem(THEME_KEY)).toBe("dark");
    expect(screen.getByRole("button", { name: "Dark theme" }).getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: "Light theme" }));
    expect(root.getAttribute("data-theme")).toBe("light");
    fireEvent.click(screen.getByRole("button", { name: "Use the system theme" }));
    expect(root.hasAttribute("data-theme")).toBe(false);
    expect(localStorage.getItem(THEME_KEY)).toBeNull();
  });

  it("works when storage throws or holds junk", () => {
    const broken = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
      removeItem: () => {
        throw new Error("blocked");
      },
    };
    expect(readTheme(broken)).toBe("system");
    expect(() => writeTheme("dark", broken)).not.toThrow();
    expect(readTheme({ getItem: () => "sepia" })).toBe("system");
    expect(readTheme({ getItem: () => "light" })).toBe("light");
  });
});

describe("index.html", () => {
  it("applies the stored choice before the app loads, with the same key", () => {
    const html = readFileSync(resolve(process.cwd(), "index.html"), "utf8");
    const script = html.match(/<script>([\s\S]*?)<\/script>/)![1];
    expect(script).toContain(`"${THEME_KEY}"`);
    for (const stored of ["dark", "light", "sepia", null]) {
      document.documentElement.removeAttribute("data-theme");
      if (stored === null) localStorage.removeItem(THEME_KEY);
      else localStorage.setItem(THEME_KEY, stored);
      new Function(script)();
      const want = stored === "dark" || stored === "light" ? stored : null;
      expect(document.documentElement.getAttribute("data-theme")).toBe(want);
    }
  });
});
