import type { ComponentChildren } from "preact";
import { cn } from "@/lib/cn";

/** The four-point star: the lodestar a navigator steers by. */
export function StarMark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" className={className}>
      <path d="M12 1.5 14.1 9.9 22.5 12 14.1 14.1 12 22.5 9.9 14.1 1.5 12 9.9 9.9Z" />
    </svg>
  );
}

/**
 * The round star mark and the serif italic wordmark (the page's only h1). `children` is the
 * small line under the wordmark.
 */
export function Logo({ children, className }: { children?: ComponentChildren; className?: string }) {
  return (
    <div className={cn("flex items-center gap-3", className)}>
      <div className="bg-mark-bg text-mark-fg grid size-10 shrink-0 place-items-center rounded-full">
        <StarMark className="size-6" />
      </div>
      <div>
        <h1 className="font-display tracking-display m-0 text-2xl leading-tight font-semibold italic">
          lodestar
        </h1>
        {children && <div className="text-muted flex items-center gap-1.5 text-[12.5px]">{children}</div>}
      </div>
    </div>
  );
}
