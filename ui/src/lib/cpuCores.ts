/**
 * Jail CPU rate as cores (decimal) vs integer percent-of-one-core on the wire.
 *
 * Spec / config store `cpu_rate_percent` where 100 = 1.00 core. The UI shows
 * cores with two fractional digits (0.01 step).
 */

/** Convert wire percent (100 = one core) to cores. */
export function percentToCores(percent: number): number {
  if (!Number.isFinite(percent)) return 0;
  return Math.max(0, percent) / 100;
}

/** Convert cores to wire percent, rounded to nearest integer percent. */
export function coresToPercent(cores: number): number {
  if (!Number.isFinite(cores) || cores <= 0) return 0;
  return Math.round(cores * 100);
}

/** Format cores for display (`0.80`, `2.00`). */
export function formatCores(cores: number): string {
  if (!Number.isFinite(cores)) return "0.00";
  return percentToCores(coresToPercent(cores)).toFixed(2);
}

/** Clamp cores into `[minCores, maxCores]` at 0.01 resolution. */
export function clampCores(cores: number, minCores: number, maxCores: number): number {
  const min = Math.max(0, minCores);
  const max = Math.max(min, maxCores);
  const stepped = Math.round(cores * 100) / 100;
  return Math.min(max, Math.max(min, stepped));
}

/** Host max cores from wire `host_cpu_rate_max` (percent). */
export function hostMaxCores(hostCpuRateMaxPercent: number): number {
  return percentToCores(Math.max(0, hostCpuRateMaxPercent));
}
