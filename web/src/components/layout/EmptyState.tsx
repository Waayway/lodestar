import type { ComponentChildren } from "preact";
import { cn } from "@/lib/cn";

/** Nothing to show yet: a quiet line in a dashed box. */
export function EmptyState({
  title,
  children,
  className,
}: {
  title: ComponentChildren;
  children?: ComponentChildren;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "border-line text-muted rounded-md border border-dashed px-4 py-12 text-center text-sm",
        className,
      )}
    >
      <p className="m-0">{title}</p>
      {children && <p className="mt-1 mb-0">{children}</p>}
    </div>
  );
}
