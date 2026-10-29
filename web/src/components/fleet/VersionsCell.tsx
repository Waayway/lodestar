import type { SoftwareView } from "@/bindings/SoftwareView";
import { Attention } from "@/icons";
import { cn } from "@/lib/cn";
import { isBehind } from "@/lib/versions";
import { toneText } from "@/lib/tones";

/**
 * The tracked software a machine reported, `name version` each. A version older than the
 * software's latest release is drawn in `warn` with an alert icon and "→ latest" beside it; unknown
 * versions stay plain. Empty when the machine reports none.
 */
export function VersionsCell({
  versions,
  software,
  className,
}: {
  versions: { [key in string]?: string };
  software: SoftwareView[];
  className?: string;
}) {
  const entries = Object.entries(versions)
    .filter((e): e is [string, string] => e[1] !== undefined)
    .sort(([a], [b]) => a.localeCompare(b));
  if (entries.length === 0) return null;
  return (
    <ul className={cn("m-0 flex list-none flex-col gap-0.5 p-0 text-[13px]", className)}>
      {entries.map(([name, version]) => {
        const latest = software.find((s) => s.name === name)?.latest ?? null;
        const behind = isBehind(version, latest);
        return (
          <li
            key={name}
            data-behind={behind}
            className="whitespace-nowrap"
          >
            <span className="text-muted">{name}</span>{" "}
            <span className={cn("font-mono tabular-nums", behind && toneText.warn)}>{version}</span>
            {behind && (
              <span className={cn("ml-1 inline-flex items-center gap-1 text-xs", toneText.warn)}>
                <Attention aria-hidden="true" className="size-3.5" />
                <span className="sr-only">Behind, latest </span>
                <span aria-hidden="true">→ </span>
                <span className="font-mono">{latest}</span>
              </span>
            )}
          </li>
        );
      })}
    </ul>
  );
}
