import * as icons from "@/icons";
import { BrandIcon, isBrand } from "@/icons/BrandIcon";

/** A Lodestar icon by its name in `@/icons` ("Machines"), or null. */
function lodestarIcon(name: string): icons.Icon | null {
  return Object.hasOwn(icons, name) ? ((icons as unknown as Record<string, icons.Icon>)[name] ?? null) : null;
}

/**
 * A service's mark on a 34 px chip (the OS-tile style): its Simple Icons brand when the slug is
 * one we carry, else the Lodestar icon of that name, else the title's first letter in the serif.
 */
export function ServiceIcon({ icon, title }: { icon: string | null; title: string }) {
  const Glyph = icon !== null && !isBrand(icon) ? lodestarIcon(icon) : null;
  return (
    <div className="bg-chip text-fg grid size-[34px] shrink-0 place-items-center rounded-sm" aria-hidden="true">
      {icon !== null && isBrand(icon) ? (
        <BrandIcon brand={icon} decorative />
      ) : Glyph ? (
        <Glyph className="size-4" />
      ) : (
        <span className="font-display text-[17px] leading-none font-semibold">{title.slice(0, 1)}</span>
      )}
    </div>
  );
}
