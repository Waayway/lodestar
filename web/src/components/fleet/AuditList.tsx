import type { AuditEvent } from "@/bindings/AuditEvent";
import { EmptyState } from "@/components/layout/EmptyState";
import { Card } from "@/components/ui/card";
import { cn } from "@/lib/cn";
import { auditLine } from "@/lib/format";
import { auditIcon, toneText } from "@/lib/tones";
import { ago } from "@/lib/time";

/** The newest audit events: who asked for what, and what the machines did. */
export function AuditList({ events, now }: { events: AuditEvent[] | null; now: number }) {
  if (events === null) return <EmptyState title="Loading the audit log" />;
  if (events.length === 0) return <EmptyState title="Nothing recorded yet." />;
  return (
    <Card className="py-1.5">
      <ul className="m-0 flex list-none flex-col p-0">
        {events.map((e) => {
          const line = auditLine(e);
          const Icon = auditIcon[line.icon];
          return (
            <li
              key={e.id}
              className="border-line-soft grid grid-cols-[28px_minmax(0,1fr)_auto] items-start gap-2.5 border-b border-dotted py-2.5 last:border-b-0"
            >
              <span className={cn("bg-chip grid size-7 place-items-center rounded-sm", toneText.accent)}>
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
