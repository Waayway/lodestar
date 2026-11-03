// The dashboard's pages, chosen by the URL hash.
import { useEffect, useState } from "preact/hooks";

export type Route = "fleet" | "builds" | "services";

export function parseRoute(hash: string): Route {
  if (hash === "#/services") return "services";
  return hash === "#/builds" ? "builds" : "fleet";
}

/** The current route; follows the browser's back/forward and link clicks. */
export function useRoute(): Route {
  const [route, setRoute] = useState<Route>(() => parseRoute(window.location.hash));
  useEffect(() => {
    const onChange = () => setRoute(parseRoute(window.location.hash));
    window.addEventListener("hashchange", onChange);
    return () => window.removeEventListener("hashchange", onChange);
  }, []);
  return route;
}
