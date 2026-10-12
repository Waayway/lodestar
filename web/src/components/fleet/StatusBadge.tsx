import type { ApplyRecord } from "@/bindings/ApplyRecord";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/cn";
import { applyBadge } from "@/lib/format";
import { applyStyle } from "@/lib/tones";

/** An apply's state: colour, icon and the reviewed label (`applyBadge`). Spins while building. */
export function StatusBadge({
  apply,
  flakePath = null,
  wrap = false,
  className,
}: {
  apply: ApplyRecord | null;
  /** The host's checkout path, named in "Skipped: local changes in …". */
  flakePath?: string | null;
  /** Let a long label wrap instead of truncating (table cells, cards). */
  wrap?: boolean;
  className?: string;
}) {
  const b = applyBadge(apply, flakePath);
  const { tone, icon: Icon, spin } = applyStyle[b.tone];
  return (
    <Badge tone={tone} data-state={b.tone} className={cn(wrap && "whitespace-normal", className)}>
      <Icon aria-hidden="true" className={spin ? "animate-spin" : undefined} />
      <span className={wrap ? "min-w-0" : "truncate"}>{b.text}</span>
    </Badge>
  );
}
