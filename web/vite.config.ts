import { readFileSync } from "node:fs";
import { fileURLToPath, URL } from "node:url";
import preact from "@preact/preset-vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vitest/config";

// The Rust server (`lodestar-server` or `lodestar-server demo`) the dev server proxies `/api` to.
const api = process.env.LODESTAR_API ?? "http://127.0.0.1:8080";

/** A package's ES module entry (its `module` field). */
function esmEntry(pkg: string): string {
  const dir = new URL(`./node_modules/${pkg}/`, import.meta.url);
  const { module } = JSON.parse(readFileSync(new URL("package.json", dir), "utf8")) as { module: string };
  return fileURLToPath(new URL(module, dir));
}

export default defineConfig({
  plugins: [preact(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
      react: "preact/compat",
      "react-dom/test-utils": "preact/test-utils",
      "react-dom": "preact/compat",
      "react/jsx-runtime": "preact/jsx-runtime",
    },
  },
  server: {
    proxy: {
      "/api": { target: api, ws: true, changeOrigin: false },
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
  test: {
    include: ["tests/**/*.test.{ts,tsx}"],
    // Bundle these through Vite in tests too, so their `react` imports hit preact/compat.
    server: { deps: { inline: [/radix-ui/, /@floating-ui/, /lucide-react/, /sonner/, /@testing-library/, /react-remove-scroll/, /react-style-singleton/, /use-callback-ref/, /use-sidecar/, /aria-hidden/] } },
    // These packages have no `exports` map, so Node would load their CommonJS builds, which
    // require real React. Point tests at their ES builds, as the browser bundle uses.
    // Only under Vitest (the config is read by builds too, where the files may be absent).
    alias: !process.env.VITEST ? [] : [
      "lucide-react",
      "react-remove-scroll",
      "react-remove-scroll-bar",
      "react-style-singleton",
      "use-callback-ref",
      "use-sidecar",
      "aria-hidden",
    ].map((pkg) => ({ find: new RegExp(`^${pkg}$`), replacement: esmEntry(pkg) })),
  },
});
