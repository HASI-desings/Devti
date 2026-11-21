/** "1.4 MB", "820 KB" — decimal units, as file managers show them. */
export function formatBytes(n: number): string {
  if (n < 1000) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n;
  let i = -1;
  do {
    v /= 1000;
    i++;
  } while (v >= 1000 && i < units.length - 1);
  return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}

/** Seconds left at the current rate, as "about 2 minutes left". */
export function etaLabel(remaining: number, rate: number): string | null {
  if (rate <= 0 || remaining <= 0) return null;
  const s = Math.round(remaining / rate);
  if (s < 5) return "a few seconds left";
  if (s < 60) return `${s} seconds left`;
  const m = Math.round(s / 60);
  return m === 1 ? "about a minute left" : `about ${m} minutes left`;
}
