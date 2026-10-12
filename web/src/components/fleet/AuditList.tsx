import type { AuditEvent } from "@/bindings/AuditEvent";
import { EmptyState } from "@/components/layout/EmptyState";
import { Card } from "@/components/ui/card";
import {
  Activation,
  Applied,
  Apply,
  Audit as AuditIcon,
  Failed,
  Promote,
  Skipped,
} from "@/icons";
import { type AuditLine, auditLine } from "@/lib/format";
import { ago } from "@/lib/time";

const icons: Record<AuditLine["icon"], typeof Apply> = {
  apply: Apply,
  applied: Applied,
  failed: Failed,
  skipped: Skipped,
  activation: Activation,
  promote: Promote,
  other: AuditIcon,
};

/** The newest audit events: who asked for what, and what the machines did. */
export function AuditList({ events, now }: { events: AuditEvent[] | null; now: number }) {
  if (events === null) return <EmptyState title="Loading the audit log" />;
  if (events.length === 0) return <EmptyState title="Nothing recorded yet." />;
  return (
    <Card className="py-1.5">
      <ul className="m-0 flex list-none flex-col p-0">
        {events.map((e) => {
          const line = auditLine(e);
          const Icon = icons[line.icon];
          return (
            <li
              key={e.id}
              className="border-line-soft grid grid-cols-[28px_minmax(0,1fr)_auto] items-start gap-2.5 border-b border-dotted py-2.5 last:border-b-0"
            >
              <span className="bg-chip text-accent grid size-7 place-items-center rounded-sm">
                <Icon aria-hidden="true" className="size-4" />
              </span>
              <div className="min-w-0">
                <div className="truncate text-[13px]">{line.title}</div>
                <div className="text-muted text-xs">{line.meta}</div>
              </div>
              <span className="text-muted text-xs whitespace-nowrap">{ago(e.time, now)}</span>
            </li>
          );
        })}
      </ul>
    </Card>
  );
}
