import { cn } from "@/lib/cn";
import { shortRev } from "@/lib/format";

/** A commit as 7 mono characters on a chip; the full hash in the tooltip. */
export function CommitHash({ rev, className }: { rev: string; className?: string }) {
  return (
    <span
      title={rev}
      className={cn("bg-chip text-fg rounded-sm px-1.5 py-0.5 font-mono text-[12.5px]", className)}
    >
      {shortRev(rev)}
    </span>
  );
}
