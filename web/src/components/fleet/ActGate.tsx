import type { ComponentChildren } from "preact";
import { useActPermission } from "@/connection/hooks";

/**
 * Wraps an action button: nothing for viewers who may never act (readers), the button
 * disabled with the reason as its tooltip for an admin who cannot act right now (off the
 * tailnet), the button as is otherwise. The wrapper carries the tooltip too, because a
 * disabled button ignores the pointer.
 */
export function ActGate({
  className,
  children,
}: {
  className?: string;
  children: (gate: { disabled: boolean; title: string | undefined }) => ComponentChildren;
}) {
  const { visible, disabledReason } = useActPermission();
  if (!visible) return null;
  const title = disabledReason ?? undefined;
  return (
    <span title={title} className={className ?? "inline-flex"}>
      {children({ disabled: disabledReason !== null, title })}
    </span>
  );
}
