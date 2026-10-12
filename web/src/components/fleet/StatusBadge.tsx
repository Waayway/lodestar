import type { ApplyRecord } from "@/bindings/ApplyRecord";
import { Badge } from "@/components/ui/badge";
import { applyBadge } from "@/lib/format";
import { applyStyle } from "@/lib/tones";

/** An apply's state: colour, icon and the reviewed label (`applyBadge`). Spins while building. */
export function StatusBadge({
  apply,
  flakePath = null,
  className,
}: {
  apply: ApplyRecord | null;
  /** The host's checkout path, named in "Skipped: local changes in …". */
  flakePath?: string | null;
  className?: string;
}) {
  const b = applyBadge(apply, flakePath);
  const { tone, icon: Icon, spin } = applyStyle[b.tone];
  return (
    <Badge tone={tone} data-state={b.tone} className={className}>
      <Icon aria-hidden="true" className={spin ? "animate-spin" : undefined} />
      <span className="truncate">{b.text}</span>
    </Badge>
  );
}
