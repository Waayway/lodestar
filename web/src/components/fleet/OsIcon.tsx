import { BrandIcon } from "@/icons/BrandIcon";
import { Machines } from "@/icons";
import { osBrand } from "@/lib/format";

/** The machine's OS mark on a 34 px chip: NixOS, Apple, or a plain monitor. */
export function OsIcon({ os }: { os: string }) {
  const brand = osBrand(os);
  return (
    <div className="bg-chip grid size-[34px] shrink-0 place-items-center rounded-sm">
      {brand ? (
        <BrandIcon brand={brand} />
      ) : (
        <Machines role="img" aria-label={os} className="text-muted size-4" />
      )}
    </div>
  );
}
