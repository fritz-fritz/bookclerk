import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { PluginConsentResponse } from "@/lib/api";
import { cn } from "@/lib/utils";

export type PluginConsentGrantDraft = {
  networkMode: string;
  domains: string[];
  bindings: string[];
  compatibilityFlags: string[];
  cpuMs?: number;
  subrequests?: number;
};

function uniqueSubset(values: string[] | undefined, allowed?: string[]): string[] {
  const allowedSet = allowed ? new Set(allowed.map((v) => v.toLowerCase())) : null;
  const out: string[] = [];
  for (const raw of values ?? []) {
    const value = raw.trim();
    if (!value) continue;
    if (allowedSet && !allowedSet.has(value.toLowerCase())) continue;
    if (!out.some((existing) => existing.toLowerCase() === value.toLowerCase())) {
      out.push(value);
    }
  }
  return out;
}

function parsePositiveInt(value: string, max?: number): number | undefined {
  const trimmed = value.trim();
  if (!trimmed) return undefined;
  const parsed = Number.parseInt(trimmed, 10);
  if (!Number.isFinite(parsed) || parsed < 1) return undefined;
  return max == null ? parsed : Math.min(parsed, max);
}

function initialChecked(requested: string[], existing?: string[]): string[] {
  const subset = uniqueSubset(existing, requested);
  return subset.length > 0 || existing ? subset : requested;
}

/**
 * Branded plugin consent review dialog with editable least-privilege grant controls.
 *
 * @param props - Consent payload, busy state, close handler, and approve callback.
 */
export function PluginConsentDialog({
  consent,
  busy,
  onCancel,
  onApprove,
}: {
  consent: PluginConsentResponse;
  busy: boolean;
  onCancel: () => void;
  onApprove: (grant: PluginConsentGrantDraft) => void | Promise<void>;
}) {
  const request = consent.request;
  const existing = consent.existing;
  const brand = consent.brand;
  const brandName = brand?.name?.trim() || request.pluginId || consent.plugin_id;
  const [networkMode, setNetworkMode] = useState(request.networkMode || "deny");
  const [domains, setDomains] = useState<string[]>([]);
  const [domainDraft, setDomainDraft] = useState("");
  const [bindings, setBindings] = useState<string[]>([]);
  const [compatibilityFlags, setCompatibilityFlags] = useState<string[]>([]);
  const [cpuMs, setCpuMs] = useState("");
  const [subrequests, setSubrequests] = useState("");

  useEffect(() => {
    setNetworkMode(existing?.networkMode || request.networkMode || "deny");
    setDomains(initialChecked(request.domains, existing?.domains));
    setBindings(initialChecked(request.bindings, existing?.bindings));
    setCompatibilityFlags(
      initialChecked(request.compatibilityFlags, existing?.compatibilityFlags),
    );
    setCpuMs(String(existing?.cpuMs ?? request.cpuMs ?? consent.limits?.cpu_ms ?? ""));
    setSubrequests(
      String(existing?.subrequests ?? request.subrequests ?? consent.limits?.subrequests ?? ""),
    );
    setDomainDraft("");
  }, [consent, existing, request]);

  const requestAllowsOutbound = request.networkMode.toLowerCase() === "outbound";
  const showDomainEditor = requestAllowsOutbound && request.domains.length > 0;
  const nativeCoarseOutbound = requestAllowsOutbound && request.domains.length === 0;
  const selectedDomains = useMemo(
    () => uniqueSubset(domains, request.domains),
    [domains, request.domains],
  );

  function toggleValue(
    value: string,
    checked: boolean,
    setter: (next: string[]) => void,
    current: string[],
  ) {
    setter(
      checked
        ? uniqueSubset([...current, value])
        : current.filter((item) => item !== value),
    );
  }

  function addDomain() {
    const value = domainDraft.trim();
    if (!value) return;
    if (!request.domains.some((domain) => domain.toLowerCase() === value.toLowerCase())) {
      return;
    }
    setDomains((current) => uniqueSubset([...current, value], request.domains));
    setDomainDraft("");
  }

  const accentStyle: CSSProperties = {
    background: brand?.accent || brand?.bg || undefined,
  };
  const headerStyle: CSSProperties = {
    background: brand?.bg || undefined,
    color: brand?.fg || undefined,
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-ink/40 px-4 py-10 sm:items-center"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !busy) onCancel();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="plugin-consent-title"
        tabIndex={-1}
        className="w-full max-w-2xl overflow-hidden rounded-lg border border-ink/10 bg-paper shadow-xl outline-none"
      >
        <div className="h-1 bg-teal" style={accentStyle} />
        <div
          className={cn(
            "flex items-start justify-between gap-4 border-b border-ink/10 p-5",
            brand?.bg ? "" : "bg-paper",
          )}
          style={headerStyle}
        >
          <div className="flex min-w-0 items-center gap-3">
            {brand?.logo ? (
              <img
                src={brand.logo}
                alt=""
                className="h-10 w-10 shrink-0 rounded bg-white object-contain p-1"
              />
            ) : (
              <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded bg-white/80 text-sm font-semibold uppercase text-ink">
                {brandName.slice(0, 2)}
              </div>
            )}
            <div className="min-w-0">
              <h2 id="plugin-consent-title" className="font-display text-xl font-semibold">
                Approve {brandName}
              </h2>
              <p className="mt-1 text-sm opacity-75">
                Review the plugin capabilities before enabling it.
              </p>
            </div>
          </div>
          <Button
            type="button"
            variant="ghost"
            className="shrink-0 px-2"
            disabled={busy}
            onClick={onCancel}
            aria-label="Close plugin consent"
          >
            <X className="h-4 w-4" />
          </Button>
        </div>

        <div className="max-h-[70vh] space-y-6 overflow-y-auto p-5">
          <section className="space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <Badge className="bg-ink/5 text-ink/60 normal-case tracking-normal">
                {request.kind || "plugin"}
              </Badge>
              {consent.covered ? (
                <Badge className="bg-teal/15 text-ink normal-case tracking-normal">Granted</Badge>
              ) : (
                <Badge className="bg-brick/10 text-brick normal-case tracking-normal">
                  Needs approval
                </Badge>
              )}
            </div>
            {consent.summary.length > 0 ? (
              <ul className="list-disc space-y-1 pl-5 text-sm text-ink/70">
                {consent.summary.map((line) => (
                  <li key={line}>{line}</li>
                ))}
              </ul>
            ) : null}
          </section>

          <section className="space-y-3">
            <div>
              <h3 className="text-sm font-semibold text-ink">Network</h3>
              <p className="text-sm text-ink/55">
                Deny is stricter. Outbound allows the plugin to reach the network.
              </p>
            </div>
            {requestAllowsOutbound ? (
              <select
                className="w-full rounded-md border border-ink/15 bg-white/80 px-3 py-2 text-sm text-ink shadow-sm focus:border-teal focus:outline-none focus:ring-2 focus:ring-teal/30"
                value={networkMode}
                disabled={busy}
                onChange={(e) => setNetworkMode(e.target.value)}
              >
                <option value="outbound">Allow outbound network</option>
                <option value="deny">Deny network</option>
              </select>
            ) : (
              <Badge className="bg-ink/5 text-ink/60 normal-case tracking-normal">
                Network denied
              </Badge>
            )}
            {nativeCoarseOutbound && networkMode === "outbound" ? (
              <p className="rounded-md border border-brick/20 bg-brick/5 px-3 py-2 text-sm text-brick">
                This native plugin requests coarse internet access. There is no hostname
                allowlist for native outbound traffic.
              </p>
            ) : null}
          </section>

          {showDomainEditor ? (
            <section className="space-y-3">
              <div>
                <h3 className="text-sm font-semibold text-ink">Allowed domains</h3>
                <p className="text-sm text-ink/55">
                  Workerd plugins can be narrowed to any subset of requested domains.
                </p>
              </div>
              <div className="flex flex-wrap gap-2">
                {request.domains.map((domain) => {
                  const checked = selectedDomains.includes(domain);
                  return (
                    <button
                      key={domain}
                      type="button"
                      disabled={busy || networkMode === "deny"}
                      className={cn(
                        "rounded-full border px-2.5 py-1 text-xs font-medium transition-colors",
                        checked
                          ? "border-teal/40 bg-teal/15 text-ink"
                          : "border-ink/10 bg-white/50 text-ink/55",
                      )}
                      onClick={() =>
                        setDomains((current) =>
                          checked
                            ? current.filter((item) => item !== domain)
                            : uniqueSubset([...current, domain], request.domains),
                        )
                      }
                    >
                      {domain}
                    </button>
                  );
                })}
              </div>
              <div className="flex gap-2">
                <Input
                  value={domainDraft}
                  disabled={busy || networkMode === "deny"}
                  onChange={(e) => setDomainDraft(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      addDomain();
                    }
                  }}
                  placeholder="Add requested domain"
                  autoComplete="off"
                  spellCheck={false}
                />
                <Button
                  type="button"
                  variant="secondary"
                  disabled={busy || networkMode === "deny" || !domainDraft.trim()}
                  onClick={addDomain}
                >
                  Add
                </Button>
              </div>
            </section>
          ) : null}

          {request.bindings.length > 0 ? (
            <section className="space-y-3">
              <h3 className="text-sm font-semibold text-ink">Host bindings</h3>
              <div className="grid gap-2 sm:grid-cols-2">
                {request.bindings.map((binding) => (
                  <label key={binding} className="flex items-center gap-2 text-sm text-ink">
                    <input
                      type="checkbox"
                      className="h-4 w-4 accent-teal"
                      checked={bindings.includes(binding)}
                      disabled={busy}
                      onChange={(e) =>
                        toggleValue(binding, e.target.checked, setBindings, bindings)
                      }
                    />
                    {binding}
                  </label>
                ))}
              </div>
            </section>
          ) : null}

          {consent.limits ? (
            <section className="space-y-3">
              <div>
                <h3 className="text-sm font-semibold text-ink">Workerd limits</h3>
                <p className="text-sm text-ink/55">
                  Lower values are allowed; server ceilings are enforced on approve.
                </p>
              </div>
              <div className="grid gap-3 sm:grid-cols-2">
                <label className="space-y-1.5 text-sm font-medium text-ink">
                  CPU milliseconds
                  <Input
                    type="number"
                    min={1}
                    max={consent.limits.max_cpu_ms}
                    value={cpuMs}
                    disabled={busy}
                    onChange={(e) => setCpuMs(e.target.value)}
                  />
                  <span className="block text-xs font-normal text-ink/50">
                    Max {consent.limits.max_cpu_ms}
                  </span>
                </label>
                <label className="space-y-1.5 text-sm font-medium text-ink">
                  Subrequests
                  <Input
                    type="number"
                    min={1}
                    max={consent.limits.max_subrequests}
                    value={subrequests}
                    disabled={busy}
                    onChange={(e) => setSubrequests(e.target.value)}
                  />
                  <span className="block text-xs font-normal text-ink/50">
                    Max {consent.limits.max_subrequests}
                  </span>
                </label>
              </div>
            </section>
          ) : null}

          {request.compatibilityFlags.length > 0 ? (
            <section className="space-y-3">
              <h3 className="text-sm font-semibold text-ink">Compatibility flags</h3>
              <div className="grid gap-2 sm:grid-cols-2">
                {request.compatibilityFlags.map((flag) => (
                  <label key={flag} className="flex items-center gap-2 text-sm text-ink">
                    <input
                      type="checkbox"
                      className="h-4 w-4 accent-teal"
                      checked={compatibilityFlags.includes(flag)}
                      disabled={busy}
                      onChange={(e) =>
                        toggleValue(
                          flag,
                          e.target.checked,
                          setCompatibilityFlags,
                          compatibilityFlags,
                        )
                      }
                    />
                    {flag}
                  </label>
                ))}
              </div>
            </section>
          ) : null}
        </div>

        <div className="flex flex-wrap justify-end gap-2 border-t border-ink/10 p-5">
          <Button type="button" variant="ghost" disabled={busy} onClick={onCancel}>
            Cancel
          </Button>
          <Button
            type="button"
            disabled={busy}
            onClick={() =>
              void onApprove({
                networkMode,
                domains: networkMode === "deny" ? [] : selectedDomains,
                bindings,
                compatibilityFlags,
                cpuMs: parsePositiveInt(cpuMs, consent.limits?.max_cpu_ms),
                subrequests: parsePositiveInt(
                  subrequests,
                  consent.limits?.max_subrequests,
                ),
              })
            }
          >
            {busy ? "Approving..." : "Approve and enable"}
          </Button>
        </div>
      </div>
    </div>
  );
}
