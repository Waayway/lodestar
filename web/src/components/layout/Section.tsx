import type { ComponentChildren } from "preact";
import type { Icon } from "@/icons";
import { cn } from "@/lib/cn";

/**
 * A part of the page under a small-caps serif heading with a rule running to the right edge,
 * like a legend on a chart.
 */
export function Section({
  title,
  icon: Glyph,
  label,
  className,
  children,
}: {
  title: ComponentChildren;
  icon?: Icon;
  /** The section's accessible name when `title` is not plain text. */
  label?: string;
  className?: string;
  children: ComponentChildren;
}) {
  return (
    <section aria-label={label} className={cn("min-w-0", className)}>
      <SectionHeading icon={Glyph}>{title}</SectionHeading>
      {children}
    </section>
  );
}

export function SectionHeading({
  icon: Glyph,
  className,
  children,
}: {
  icon?: Icon;
  className?: string;
  children: ComponentChildren;
}) {
  return (
    <h2
      className={cn(
        "font-display text-muted tracking-label mt-0 mb-2.5 flex items-center gap-2 text-[13px] font-semibold uppercase",
        "after:border-line after:ml-1.5 after:flex-1 after:border-t after:content-['']",
        className,
      )}
    >
      {Glyph && <Glyph aria-hidden="true" className="size-4" />}
      {children}
    </h2>
  );
}
