// Relative times and durations. Times are unix seconds.
import { useEffect, useState } from "preact/hooks";

/** "5m ago", "2h ago", "3d ago"; "just now" under a minute. */
export function ago(time: number, now: number): string {
  const s = Math.max(0, now - time);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

/** "45s" or "2m 05s": how long an apply ran (or has been running). */
export function durationLabel(start: number | null, end: number | null, now: number): string {
  if (start === null) return "";
  const s = Math.max(0, (end ?? now) - start);
  if (s < 60) return `${s}s`;
  return `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
}

/** The current time in unix seconds, refreshed every `intervalMs`. */
export function useNow(intervalMs = 15_000): number {
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  useEffect(() => {
    const t = setInterval(() => setNow(Math.floor(Date.now() / 1000)), intervalMs);
    return () => clearInterval(t);
  }, [intervalMs]);
  return now;
}
