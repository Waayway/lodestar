import { useState } from "preact/hooks";
import type { UpdatesView } from "@/bindings/UpdatesView";
import { ActGate } from "@/components/fleet/ActGate";
import { CommitHash } from "@/components/fleet/CommitHash";
import { Button } from "@/components/ui/button";
import { Card, CardHeader } from "@/components/ui/card";
import { useActions } from "@/connection/hooks";
import { Building } from "@/icons";
import { cn } from "@/lib/cn";
import { updateResultLabel } from "@/lib/format";
import { ago } from "@/lib/time";
import { toneChip, updateResultStyle } from "@/lib/tones";

/**
 * Tracked software and the updater: the latest release of each, the last run per flake input
 * (result, age, commit, what got newer), and the "Check now" button.
 */
export function UpdatesCard({ updates, now }: { updates: UpdatesView; now: number }) {
  const { runUpdates } = useActions();
  const [sending, setSending] = useState(false);
  const running = updates.running || sending;

  async function check() {
    if (running) return;
    setSending(true);
    try {
      await runUpdates();
    } finally {
      setSending(false);
    }
  }

  return (
    <Card className="min-[900px]:col-span-full">
      <CardHeader>
        <strong>Software updates</strong>
        <ActGate>
          {(gate) => (
            <Button size="sm" variant="outline" disabled={gate.disabled || running} title={gate.title} onClick={check}>
              {running && <Building aria-hidden="true" className="size-4 animate-spin" />}
              {running ? "Running…" : "Check now"}
            </Button>
          )}
        </ActGate>
      </CardHeader>
      <ul className="m-0 flex list-none flex-col gap-1 p-0 text-[13px]">
        {updates.software.map((s) => (
          <li key={s.name} className="flex flex-wrap items-baseline gap-x-2">
            <span className="font-semibold">{s.name}</span>
            {s.latest !== null && <span className="font-mono">{s.latest}</span>}
            {s.error !== null ? (
              <span className="text-bad">Check failed: {s.error}</span>
            ) : (
              s.checked_at !== null && <span className="text-muted text-xs">checked {ago(s.checked_at, now)}</span>
            )}
          </li>
        ))}
      </ul>
      {updates.inputs.length > 0 && (
        <ul className="border-line-soft m-0 flex list-none flex-col gap-2 border-t p-0 pt-2.5 text-[13px]">
          {updates.inputs.map((i) => {
            const last = i.last;
            const style = last && updateResultStyle(last.result);
            return (
              <li key={i.name} data-input={i.name} className="flex flex-col gap-1">
                <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
                  <span className="font-mono">{i.name}</span>
                  {last && style ? (
                    <>
                      <span
                        className={cn(
                          "inline-flex items-center gap-1 rounded-sm px-1.5 py-px text-[12.5px]",
                          toneChip(style.tone),
                        )}
                      >
                        <style.icon aria-hidden="true" className="size-3.5" />
                        {updateResultLabel(last.result)}
                      </span>
                      <span className="text-muted text-xs">{ago(last.finished_at, now)}</span>
                      {last.commit && <CommitHash rev={last.commit} />}
                    </>
                  ) : (
                    <span className="text-muted text-xs">Never run</span>
                  )}
                </div>
                {last && last.changes.length > 0 && (
                  <div className="text-muted text-xs">
                    {last.changes.map((c) => `${c.software} ${c.from} → ${c.to}`).join(", ")}
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </Card>
  );
}
