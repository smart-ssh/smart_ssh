/** Issue #623: speed and remaining time of running transfers (Spec 0054). */

/** Moving-average window for the speed. */
export const RATE_WINDOW_MS = 5000;
/** The shown values change at most this often (~2 times per second). */
export const RATE_DISPLAY_INTERVAL_MS = 500;

export interface RateState {
  /** Progress samples inside the window (oldest first). */
  samples: { t: number; bytes: number }[];
  /** Shown speed in bytes/s; null while unknown. */
  speed: number | null;
  /** Shown remaining seconds; null while unknown. */
  etaSeconds: number | null;
  /** Time the shown values were last refreshed. */
  shownAt: number;
}

/** Folds one progress event (at `now`, ms) into the rate state. The samples
 * always take the event; the shown speed/ETA are refreshed at most every
 * `RATE_DISPLAY_INTERVAL_MS`. */
export function nextRate(
  prev: RateState | undefined,
  now: number,
  bytesDone: number,
  totalBytes: number | null,
): RateState {
  const samples = [...(prev?.samples ?? []), { t: now, bytes: bytesDone }].filter(
    (s) => now - s.t <= RATE_WINDOW_MS,
  );
  if (prev && now - prev.shownAt < RATE_DISPLAY_INTERVAL_MS) {
    return { ...prev, samples };
  }
  const first = samples[0];
  const dt = (now - first.t) / 1000;
  const speed = dt > 0 ? Math.max(0, (bytesDone - first.bytes) / dt) : null;
  const etaSeconds =
    speed !== null && speed > 0 && totalBytes !== null && totalBytes >= bytesDone
      ? Math.ceil((totalBytes - bytesDone) / speed)
      : null;
  return { samples, speed, etaSeconds, shownAt: now };
}

/** `KB/s` / `MB/s`; "—" while unknown. */
export function formatSpeed(bytesPerSecond: number | null): string {
  if (bytesPerSecond === null) return "—";
  if (bytesPerSecond < 1024 * 1024) return `${(bytesPerSecond / 1024).toFixed(1)} KB/s`;
  return `${(bytesPerSecond / (1024 * 1024)).toFixed(1)} MB/s`;
}

/** `mm:ss` or `h:mm:ss`; "—" while unknown. */
export function formatDuration(seconds: number | null): string {
  if (seconds === null) return "—";
  const s = Math.max(0, Math.round(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${String(m).padStart(2, "0")}:${ss}`;
}
