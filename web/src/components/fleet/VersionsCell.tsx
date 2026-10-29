import type { SoftwareView } from "@/bindings/SoftwareView";
import { cn } from "@/lib/cn";
import { isBehind } from "@/lib/versions";
import { toneText } from "@/lib/tones";

/**
 * The tracked software a machine reported, `name version` each. A version older than the
 * software's latest release is drawn in `warn` with the latest in its tooltip; unknown
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
            title={behind ? `Latest is ${latest}` : undefined}
            className="whitespace-nowrap"
          >
            <span className="text-muted">{name}</span>{" "}
            <span className={cn("font-mono tabular-nums", behind && toneText.warn)}>{version}</span>
          </li>
        );
      })}
    </ul>
  );
}
