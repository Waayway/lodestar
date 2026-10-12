// OS / brand marks from Simple Icons. Only the named exports are imported, so the bundle
// carries these two paths and not the whole set.
import { siApple, siNixos } from "simple-icons";
import { cn } from "@/lib/cn";

const brands = {
  nixos: { icon: siNixos, label: "NixOS", className: "text-nix" },
  apple: { icon: siApple, label: "macOS", className: "" },
} as const;

export type Brand = keyof typeof brands;

/**
 * A brand mark at icon size (16 px unless `className` sets a size). NixOS is drawn in
 * `--nix`, Apple in the current text colour. Labelled for screen readers unless `decorative`.
 */
export function BrandIcon({
  brand,
  className,
  decorative = false,
}: {
  brand: Brand;
  className?: string;
  decorative?: boolean;
}) {
  const b = brands[brand];
  return (
    <svg
      viewBox="0 0 24 24"
      fill="currentColor"
      className={cn("size-4 shrink-0", b.className, className)}
      role={decorative ? undefined : "img"}
      aria-label={decorative ? undefined : b.label}
      aria-hidden={decorative ? true : undefined}
      data-brand={brand}
    >
      <path d={b.icon.path} />
    </svg>
  );
}
