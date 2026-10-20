import { useState } from "preact/hooks";
import type { ChannelView } from "@/bindings/ChannelView";
import type { PromoteChannels } from "@/bindings/PromoteChannels";
import { ActGate } from "@/components/fleet/ActGate";
import { CommitHash } from "@/components/fleet/CommitHash";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { useActions, useActPermission } from "@/connection/hooks";
import { Promote } from "@/icons";
import { shortRev } from "@/lib/format";

/** Between the two channel cards: what can be promoted, and the button that asks first. */
export function PromoteCard({
  candidate,
  pair,
  channels,
}: {
  /** The commit on `pair.from` that `pair.to` can fast-forward to. */
  candidate: string;
  pair: PromoteChannels;
  channels: ChannelView[];
}) {
  return (
    <div className="border-accent-line bg-panel text-accent flex min-w-[170px] flex-col items-center justify-center gap-2 rounded-md border p-3 text-center">
      <Promote aria-hidden="true" className="size-[22px]" />
      <strong className="text-fg">Ready to promote</strong>
      <small className="text-muted text-xs">
        <span className="font-mono">{pair.from}</span> → <span className="font-mono">{pair.to}</span>
      </small>
      <PromoteDialog candidate={candidate} pair={pair} channels={channels} />
    </div>
  );
}

/**
 * "Promote from → to": fast-forwards `to` to the candidate after a confirmation. The commit
 * is fixed when the dialog opens, so a commit landing on `from` meanwhile can't change what
 * the confirmation promotes.
 */
export function PromoteDialog({
  candidate,
  pair,
  channels,
}: {
  candidate: string;
  pair: PromoteChannels;
  channels: ChannelView[];
}) {
  const { promote } = useActions();
  const act = useActPermission();
  // What the open dialog promotes, captured when it opened.
  const [target, setTarget] = useState<{ commit: string; subject: string | null } | null>(null);
  const [sending, setSending] = useState(false);
  const subjectOf = (commit: string) => channels.find((c) => c.head === commit)?.head_subject ?? null;
  const shown = target?.commit ?? candidate;
  const subject = target ? target.subject : subjectOf(candidate);
  const targetHead = channels.find((c) => c.name === pair.to)?.head ?? null;

  async function confirm() {
    if (target === null) return;
    setSending(true);
    try {
      if (await promote(target.commit, pair)) setTarget(null);
    } finally {
      setSending(false);
    }
  }

  return (
    <Dialog
      open={target !== null}
      onOpenChange={(o) => {
        if (sending) return;
        // A disabled trigger cannot open it; this keeps any other way in shut too.
        if (o && (!act.visible || act.disabledReason !== null)) return;
        setTarget(o ? { commit: candidate, subject: subjectOf(candidate) } : null);
      }}
    >
      <ActGate>
        {(gate) => (
          <DialogTrigger asChild>
            <Button size="sm" disabled={gate.disabled} title={gate.title}>
              Promote {shortRev(candidate)}
            </Button>
          </DialogTrigger>
        )}
      </ActGate>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            Promote {pair.from} → {pair.to}?
          </DialogTitle>
          <DialogDescription>
            Fast-forwards <span className="font-mono">{pair.to}</span> to this commit and pushes
            it. Machines on {pair.to} change only when you apply.
          </DialogDescription>
        </DialogHeader>
        <div className="border-line bg-bg rounded-sm border p-3">
          <div className="flex items-baseline gap-2">
            <CommitHash rev={shown} className="font-semibold" />
            <span className="min-w-0 break-words">{subject ?? "(subject unavailable)"}</span>
          </div>
          {targetHead && (
            <div className="text-muted mt-1 text-xs">
              {pair.to} is now at <span className="font-mono">{shortRev(targetHead)}</span>
            </div>
          )}
        </div>
        <DialogFooter>
          <DialogClose asChild>
            <Button variant="outline" disabled={sending}>
              Cancel
            </Button>
          </DialogClose>
          <Button onClick={confirm} disabled={sending}>
            {sending ? "Promoting" : `Promote ${shortRev(shown)}`}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
