import type { PublicService } from "@/bindings/PublicService";
import { ServiceIcon } from "@/components/services/ServiceIcon";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/cn";
import { publicServiceLabel, stateSince } from "@/lib/format";
import { publicServiceStyle, toneStripe } from "@/lib/tones";

/**
 * One service for an anonymous visitor: its mark, the title as plain text (no link, URL or
 * host), what it is, its state and how long it has held. The left-edge stripe is the tone.
 */
export function PublicServiceCard({ service: s, now }: { service: PublicService; now: number }) {
  const { tone, icon: Icon } = publicServiceStyle(s.state);
  const sub = stateSince(s.state, s.since, now);
  return (
    <li
      data-tone={tone}
      className={cn("border-line bg-panel flex min-w-0 flex-col gap-2.5 rounded-md border p-4", toneStripe[tone])}
    >
      <div className="flex min-w-0 items-start gap-3">
        <ServiceIcon icon={s.icon} title={s.title} />
        <span className="min-w-0 flex-1 truncate text-sm font-semibold">{s.title}</span>
      </div>
      {s.description && <p className="text-muted m-0 min-w-0 text-[12px]">{s.description}</p>}
      <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1.5">
        <Badge tone={tone} data-state={s.state}>
          <Icon aria-hidden="true" />
          <span>{publicServiceLabel(s.state)}</span>
        </Badge>
        {sub && <span className="text-muted text-[12px]">{sub}</span>}
      </div>
    </li>
  );
}
