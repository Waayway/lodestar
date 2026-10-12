import { cn } from "@/lib/cn";
import type { ChannelRole } from "@/lib/format";

const roles: Record<ChannelRole, string> = {
  stable: "text-stable border-stable-line",
  unstable: "text-unstable border-unstable-line",
  other: "text-fg border-line",
};

/**
 * A channel name in a mono pill. The promote target is drawn as `stable` (ink), its source
 * as `unstable` (ochre), any other channel plain.
 */
export function ChannelPill({
  name,
  role,
  className,
}: {
  name: string;
  role: ChannelRole;
  className?: string;
}) {
  return (
    <span
      data-role={role}
      className={cn(
        "inline-flex items-center gap-[5px] rounded-full border px-2 py-[3px] font-mono text-xs",
        roles[role],
        className,
      )}
    >
      {name}
    </span>
  );
}
