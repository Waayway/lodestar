import type { ApplyRecord } from "@/bindings/ApplyRecord";
import { Badge, type BadgeTone } from "@/components/ui/badge";
import { Applied, Building, Failed, Idle, Skipped } from "@/icons";
import { type ApplyTone, applyBadge } from "@/lib/format";

const badgeTone: Record<ApplyTone, BadgeTone> = {
  ok: "ok",
  running: "running",
  skipped: "skipped",
  failed: "failed",
  idle: "neutral",
};

const icons: Record<ApplyTone, typeof Applied> = {
  ok: Applied,
  running: Building,
  skipped: Skipped,
  failed: Failed,
  idle: Idle,
};

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
  const Icon = icons[b.tone];
  return (
    <Badge tone={badgeTone[b.tone]} data-state={b.tone} className={className}>
      <Icon aria-hidden="true" className={b.tone === "running" ? "animate-spin" : undefined} />
      <span className="truncate">{b.text}</span>
    </Badge>
  );
}
