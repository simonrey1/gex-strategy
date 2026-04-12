const TZ = "America/New_York";

const fmtTimeET = new Intl.DateTimeFormat("en-US", {
  timeZone: TZ,
  hour: "numeric",
  minute: "2-digit",
  hour12: true,
});

const fmtDateTimeShortET = new Intl.DateTimeFormat("en-US", {
  timeZone: TZ,
  year: "2-digit",
  month: "2-digit",
  day: "2-digit",
  hour: "numeric",
  minute: "2-digit",
  hour12: false,
});

const fmtDateTimeLongET = new Intl.DateTimeFormat("en-US", {
  timeZone: TZ,
  month: "2-digit",
  day: "2-digit",
  hour: "numeric",
  minute: "2-digit",
  second: "2-digit",
  hour12: true,
});

/** Format a Date or ISO string as "3:30 PM ET" (time only) */
export function etTime(d: Date | string): string {
  const date = typeof d === "string" ? new Date(d) : d;
  return fmtTimeET.format(date) + " ET";
}

/** Format a Date or ISO string as "03/04, 15:30" (short date+time, 24h) — for trade tables */
export function etShort(d: Date | string): string {
  const date = typeof d === "string" ? new Date(d) : d;
  return fmtDateTimeShortET.format(date);
}

/** Format a Date or ISO string as "03/04, 3:30:00 PM" (full date+time) — for trade log */
export function etFull(d: Date | string): string {
  const date = typeof d === "string" ? new Date(d) : d;
  return fmtDateTimeLongET.format(date);
}

/**
 * Shift a UTC Unix-seconds timestamp to Eastern Time for lightweight-charts.
 * lightweight-charts treats all times as UTC internally, so we subtract the
 * ET offset to make the wall-clock time appear correct.
 *
 * EDT runs from the second Sunday of March at 07:00 UTC to the first Sunday
 * of November at 06:00 UTC. The range table is computed at module load time
 * so it works for any year without magic numbers.
 */
function nthSunday(year: number, month: number, n: number): number {
  const d = new Date(Date.UTC(year, month, 1));
  const firstDow = d.getUTCDay();
  const dayOffset = (7 - firstDow) % 7;
  d.setUTCDate(1 + dayOffset + (n - 1) * 7);
  return d.getTime() / 1000;
}

function buildEdtRanges(fromYear: number, toYear: number): [number, number][] {
  const ranges: [number, number][] = [];
  for (let y = fromYear; y <= toYear; y++) {
    const start = nthSunday(y, 2, 2) + 7 * 3600;  // 2nd Sunday of March, 07:00 UTC
    const end = nthSunday(y, 10, 1) + 6 * 3600;    // 1st Sunday of November, 06:00 UTC
    ranges.push([start, end]);
  }
  return ranges;
}

const EDT_RANGES = buildEdtRanges(2000, 2050);

export function shiftToET(utcSeconds: number): number {
  for (const [s, e] of EDT_RANGES) {
    if (utcSeconds >= s && utcSeconds < e) {
      return utcSeconds - 4 * 3600; // EDT: UTC-4
    }
  }
  return utcSeconds - 5 * 3600; // EST: UTC-5
}

export function fmtUsd(n: number | null | undefined): string {
  if (n == null) return "—";
  const sign = n >= 0 ? "" : "-";
  return `${sign}$${Math.abs(n).toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
}

export function fmtPct(n: number | null | undefined): string {
  if (n == null) return "—";
  return `${n >= 0 ? "+" : ""}${n.toFixed(2)}%`;
}

export function fmtSignedUsd(n: number): string {
  return `${n >= 0 ? "+" : ""}$${n.toFixed(0)}`;
}


function fmtElapsedSec(totalSec: number): string {
  if (totalSec < 60) return `${totalSec}s`;
  const totalMin = Math.floor(totalSec / 60);
  if (totalMin < 60) return `${totalMin}m`;
  const h = Math.floor(totalMin / 60);
  const m = totalMin % 60;
  if (h < 24) return m > 0 ? `${h}h ${m}m` : `${h}h`;
  const d = Math.floor(h / 24);
  const rh = h % 24;
  return rh > 0 ? `${d}d ${rh}h` : `${d}d`;
}

export function fmtDuration(entryIso: string, exitIso: string): string {
  const sec = Math.floor((new Date(exitIso).getTime() - new Date(entryIso).getTime()) / 1000);
  return fmtElapsedSec(sec);
}

export const fmtUptime = fmtElapsedSec;

export function fmtWall(v: number | null | undefined): string {
  return v != null ? `$${v.toFixed(0)}` : "—";
}

export function pnlClass(n: number | null | undefined): string {
  if (n == null) return "";
  return n > 0 ? "positive" : n < 0 ? "negative" : "";
}

export function ago(ms: number | null): string {
  if (!ms) return "—";
  const s = Math.round((Date.now() - ms) / 1000);
  if (s < 60) return `${s}s ago`;
  if (s < 3600) return `${Math.round(s / 60)}m ago`;
  return `${Math.round(s / 3600)}h ago`;
}

export function pollHealthClass(ms: number | null): "healthy" | "warn" | "stale" {
  if (!ms) return "stale";
  const s = (Date.now() - ms) / 1000;
  if (s < 1020) return "healthy";  // within 1 poll cycle (15m + 2m buffer)
  if (s < 2100) return "warn";     // missed 1 cycle
  return "stale";
}

export function fmtNumber(n: number): string {
  return n.toLocaleString();
}
