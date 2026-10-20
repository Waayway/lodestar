// What the viewer is and may do, derived from the server's `/api/me` answer. Pure helpers; the
// connection layer fetches `Me`, components ask these.
import type { Me } from "@/bindings/Me";

/** Which of the dashboard's states to render. */
export type ViewMode = "loading" | "anonymous" | "signed_in" | "forbidden";

/** How often the public feed (anonymous visitors, no WebSocket) is fetched again. */
export const PUBLIC_POLL_MS = 30_000;

export function viewMode(me: Me | null, forbidden: boolean): ViewMode {
  if (forbidden) return "forbidden";
  if (me === null) return "loading";
  return me.role === "anonymous" ? "anonymous" : "signed_in";
}

/** Whether action buttons show, and why they are disabled (a tooltip) when they show. */
export function actPermission(me: Me | null): { visible: boolean; disabledReason: string | null } {
  if (me === null || me.role !== "admin") return { visible: false, disabledReason: null };
  return { visible: true, disabledReason: me.can_act ? null : "Only over Tailscale" };
}
