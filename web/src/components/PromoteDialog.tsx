import { useState } from "preact/hooks";
import { toast } from "sonner";
import { promote } from "@/api";
import type { ChannelView } from "@/bindings/ChannelView";
import type { PromoteChannels } from "@/bindings/PromoteChannels";
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
import { shortRev } from "@/format";

/**
 * "Promote from → to" (by default unstable → stable): fast-forwards `to` to `commit` after a
 * confirmation. The commit is fixed when the dialog opens, so a commit landing on `from`
 * meanwhile can't change what the confirmation promotes.
 */
export function PromoteDialog({
  commit: candidate,
  pair,
  channels,
  onError,
}: {
  commit: string;
  pair: PromoteChannels;
  channels: ChannelView[];
  onError: (what: string, e: unknown) => void;
}) {
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
      await promote(target.commit);
      toast.success(`${pair.to} moved to ${shortRev(target.commit)}`);
      setTarget(null);
    } catch (e) {
      onError("Promote failed", e);
    } finally {
      setSending(false);
    }
  }

  return (
    <Dialog
      open={target !== null}
      onOpenChange={(o) => {
        if (!sending) setTarget(o ? { commit: candidate, subject: subjectOf(candidate) } : null);
      }}
    >
      <DialogTrigger asChild>
        <Button size="sm" className="self-start">
          Promote {pair.from} → {pair.to}
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            Promote {pair.from} → {pair.to}?
          </DialogTitle>
          <DialogDescription>
            Fast-forwards <span className="font-mono">{pair.to}</span> to this commit and pushes
            it. Hosts on {pair.to} change only when you apply.
          </DialogDescription>
        </DialogHeader>
        <div className="bg-muted/50 rounded-md border p-3 text-sm">
          <div className="flex items-baseline gap-2">
            <span className="font-mono font-semibold">{shortRev(shown)}</span>
            <span className="min-w-0 break-words">{subject ?? "(subject unavailable)"}</span>
          </div>
          {targetHead && (
            <div className="text-muted-foreground mt-1 text-xs">
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
