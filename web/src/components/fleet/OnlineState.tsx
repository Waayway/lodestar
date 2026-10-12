import type { HostView } from "@/bindings/HostView";
import { cn } from "@/lib/cn";
import { onlineLabel } from "@/lib/format";
import { onlineStyle, toneFill, toneText } from "@/lib/tones";

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
  const { tone, icon: Icon } = onlineStyle[host.online];
  return (
    <span
      data-state={host.online}
      className={cn(
        "inline-flex items-center gap-[7px] text-[13px] whitespace-nowrap",
        // Online keeps the ink for its words; the dot carries the colour.
        host.online !== "on" && toneText[tone],
        className,
      )}
    >
      {Icon ? (
        <Icon aria-hidden="true" className="size-4" />
      ) : (
        <span aria-hidden="true" className={cn("outline-ok-soft size-[9px] shrink-0 rounded-full outline-2", toneFill[tone])} />
      )}
      {onlineLabel(host, now)}
    </span>
  );
}
