import type { HostView } from "@/bindings/HostView";
import { Off, Unreachable } from "@/icons";
import { cn } from "@/lib/cn";
import { onlineLabel } from "@/lib/format";

/** Whether a machine is on and its agent answers: a dot, or an icon, and the label. */
export function OnlineState({
  host,
  now,
  className,
}: {
  host: Pick<HostView, "online" | "last_seen">;
  now: number;
  className?: string;
}) {
  const label = onlineLabel(host as HostView, now);
  return (
    <span
      data-state={host.online}
      className={cn(
        "inline-flex items-center gap-[7px] text-[13px] whitespace-nowrap",
        host.online === "agent_unreachable" && "text-warn",
        host.online === "off" && "text-muted",
        className,
      )}
    >
      {host.online === "on" && (
        <span aria-hidden="true" className="bg-ok outline-ok-soft size-[9px] shrink-0 rounded-full outline-2" />
      )}
      {host.online === "agent_unreachable" && <Unreachable aria-hidden="true" className="size-4" />}
      {host.online === "off" && <Off aria-hidden="true" className="size-4" />}
      {label}
    </span>
  );
}
