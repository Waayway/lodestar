// OS / brand marks from Simple Icons. Only the named exports are imported, so the bundle
// carries these paths and not the whole set.
import { siApple, siImmich, siJellyfin, siNextcloud, siNixos, siProxmox, siTraefikproxy, siTruenas } from "simple-icons";
import { cn } from "@/lib/cn";

const brands = {
  nixos: { icon: siNixos, label: "NixOS", className: "text-nix" },
  apple: { icon: siApple, label: "macOS", className: "" },
  // Service icons (the Services tab), all in the current text colour.
  jellyfin: { icon: siJellyfin, label: "Jellyfin", className: "" },
  nextcloud: { icon: siNextcloud, label: "Nextcloud", className: "" },
  proxmox: { icon: siProxmox, label: "Proxmox", className: "" },
  truenas: { icon: siTruenas, label: "TrueNAS", className: "" },
  traefikproxy: { icon: siTraefikproxy, label: "Traefik Proxy", className: "" },
  immich: { icon: siImmich, label: "Immich", className: "" },
} as const;

export type Brand = keyof typeof brands;

/** Whether a slug (a link's `icon`) is one of the brand marks above. */
export function isBrand(slug: string): slug is Brand {
  return Object.hasOwn(brands, slug);
}

/**
 * A brand mark at icon size (16 px unless `className` sets a size). NixOS is drawn in
 * `--nix`, the others in the current text colour. Labelled for screen readers unless
 * `decorative`.
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
