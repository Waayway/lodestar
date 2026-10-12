import { cn } from "@/lib/cn";
import { behindLabel } from "@/lib/format";
import { behindTone, toneText } from "@/lib/tones";

/** How many commits a machine is behind its channel: green at zero, amber above. */
export function BehindCount({ behind, className }: { behind: number | null; className?: string }) {
  return (
    <span
      className={cn(
        "text-[13px] whitespace-nowrap tabular-nums",
        toneText[behindTone(behind)],
        className,
      )}
    >
      {behindLabel(behind)}
    </span>
  );
}
