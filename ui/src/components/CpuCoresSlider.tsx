import { useId } from "react";
import { cn } from "@/lib/utils";
import {
  clampCores,
  formatCores,
  hostMaxCores,
  percentToCores,
} from "@/lib/cpuCores";

export type CpuCoresSliderProps = {
  /** Current value in cores (0.01 resolution). */
  value: number;
  /** Called with cores when the operator moves the slider or types. */
  onChange: (cores: number) => void;
  /** Host max as wire percent (`logical_cpus × 100`). */
  hostMaxPercent: number;
  /** Manifest / baseline rate as wire percent (marker). */
  manifestPercent?: number | null;
  /** Global `[plugins.jail]` ceiling as wire percent (marker). */
  globalPercent?: number | null;
  /** Minimum selectable cores (default 0.01). */
  minCores?: number;
  disabled?: boolean;
  id?: string;
  className?: string;
};

/**
 * Jail CPU control as cores (two decimal places) on a 0→host-max slider.
 *
 * Markers show the plugin manifest baseline and optional global Settings cap.
 */
export function CpuCoresSlider({
  value,
  onChange,
  hostMaxPercent,
  manifestPercent,
  globalPercent,
  minCores = 0.01,
  disabled,
  id,
  className,
}: CpuCoresSliderProps) {
  const autoId = useId();
  const controlId = id ?? autoId;
  const hostMax = Math.max(minCores, hostMaxCores(hostMaxPercent));
  const globalCap =
    globalPercent != null && globalPercent > 0
      ? percentToCores(globalPercent)
      : null;
  const selectableMax =
    globalCap != null ? Math.min(hostMax, globalCap) : hostMax;
  const manifest =
    manifestPercent != null && manifestPercent > 0
      ? percentToCores(manifestPercent)
      : null;
  const cores = clampCores(value, minCores, selectableMax);

  function markLeft(coresAt: number): string {
    const pct = (coresAt / hostMax) * 100;
    return `${Math.min(100, Math.max(0, pct))}%`;
  }

  return (
    <div className={cn("flex flex-col gap-2", className)}>
      <div className="flex items-baseline justify-between gap-3">
        <label htmlFor={controlId} className="text-sm font-medium text-ink">
          CPU cores
        </label>
        <div className="flex items-center gap-2">
          <input
            type="number"
            inputMode="decimal"
            step={0.01}
            min={minCores}
            max={selectableMax}
            disabled={disabled}
            value={formatCores(cores)}
            aria-label="CPU cores"
            className="w-24 rounded-md border border-ink/15 bg-white/80 px-2 py-1 text-right text-sm tabular-nums text-ink shadow-sm focus:border-teal focus:outline-none focus:ring-2 focus:ring-teal/30"
            onChange={(e) => {
              const next = Number.parseFloat(e.target.value);
              if (!Number.isFinite(next)) return;
              onChange(clampCores(next, minCores, selectableMax));
            }}
          />
          <span className="text-xs text-ink/50">cores</span>
        </div>
      </div>

      <div className="relative pt-1 pb-5">
        <input
          id={controlId}
          type="range"
          min={minCores}
          max={hostMax}
          step={0.01}
          disabled={disabled}
          value={cores}
          aria-valuemin={minCores}
          aria-valuemax={selectableMax}
          aria-valuenow={cores}
          aria-valuetext={`${formatCores(cores)} cores`}
          className={cn(
            "h-2 w-full cursor-pointer appearance-none rounded-full bg-ink/10 accent-teal",
            "disabled:cursor-not-allowed disabled:opacity-50",
            "[&::-webkit-slider-thumb]:size-4 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-teal",
            "[&::-moz-range-thumb]:size-4 [&::-moz-range-thumb]:rounded-full [&::-moz-range-thumb]:border-0 [&::-moz-range-thumb]:bg-teal",
          )}
          onChange={(e) => {
            const raw = Number.parseFloat(e.target.value);
            onChange(clampCores(raw, minCores, selectableMax));
          }}
        />

        {/* Marker rail uses full host span so caps are visible even when selection is clamped. */}
        <div className="pointer-events-none absolute inset-x-0 top-1 h-2">
          {manifest != null ? (
            <span
              title={`Manifest ${formatCores(manifest)}`}
              className="absolute top-0 h-2 w-0.5 -translate-x-1/2 bg-ink/45"
              style={{ left: markLeft(manifest) }}
            />
          ) : null}
          {globalCap != null ? (
            <span
              title={`Global cap ${formatCores(globalCap)}`}
              className="absolute top-0 h-2 w-0.5 -translate-x-1/2 bg-teal"
              style={{ left: markLeft(globalCap) }}
            />
          ) : null}
        </div>

        <div className="absolute inset-x-0 top-5 flex justify-between text-[11px] leading-tight text-ink/50">
          <span>0.00</span>
          <span className="text-center">
            {manifest != null ? (
              <span className="block">Manifest {formatCores(manifest)}</span>
            ) : null}
            {globalCap != null ? (
              <span className="block text-teal">Global {formatCores(globalCap)}</span>
            ) : (
              <span className="block">No global cap</span>
            )}
          </span>
          <span>Host {formatCores(hostMax)}</span>
        </div>
      </div>
    </div>
  );
}
