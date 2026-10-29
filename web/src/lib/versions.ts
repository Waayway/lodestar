// Version comparison for the "behind" marks. The same rule as the server's: strip a leading
// `v`, cut at the first `-` or `+`, compare dot parts numerically with missing parts as 0;
// anything non-numeric is unknown.

function parts(v: string): number[] | null {
  const core = v.replace(/^v/, "").split(/[-+]/, 1)[0];
  const out: number[] = [];
  for (const p of core.split(".")) {
    if (!/^\d+$/.test(p)) return null;
    out.push(Number(p));
  }
  return out;
}

/** -1, 0 or 1 like a comparator; null when either side is not a numeric version. */
export function compareVersions(a: string, b: string): -1 | 0 | 1 | null {
  const x = parts(a);
  const y = parts(b);
  if (x === null || y === null) return null;
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    const d = (x[i] ?? 0) - (y[i] ?? 0);
    if (d !== 0) return d < 0 ? -1 : 1;
  }
  return 0;
}

/** Whether `running` is older than `latest`; false when there is no latest or it is unknown. */
export function isBehind(running: string, latest: string | null | undefined): boolean {
  return latest != null && compareVersions(running, latest) === -1;
}
