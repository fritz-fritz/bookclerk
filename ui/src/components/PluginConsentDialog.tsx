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
  diskMib?: number;
  memoryMib?: number;
  cpuRatePercent?: number;
  maxProcesses?: number;
};

function uniqueValues(values: string[] | undefined): string[] {
  const out: string[] = [];
  for (const raw of values ?? []) {
    const value = raw.trim();
    if (!value) continue;
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

function initialList(requested: string[], existing?: string[]): string[] {
  if (existing && existing.length > 0) return uniqueValues(existing);
  if (existing) return [];
  return uniqueValues(requested);
}

/**
 * Branded plugin consent review dialog with editable operator grant controls.
 *
 * Operators may widen or narrow the manifest baseline. Host hard caps still
 * apply. Domain allowlists are enforced for workerd guests only.
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
  const limits = consent.limits;
  const isWorkerd = consent.runtime === "workerd";
  const brandName = brand?.name?.trim() || request.pluginId || consent.plugin_id;
  const knownBindings = uniqueValues([
    ...(limits.known_bindings ?? []),
    ...request.bindings,
    ...(existing?.bindings ?? []),
  ]);
  const [networkMode, setNetworkMode] = useState(request.networkMode || "deny");
  const [domains, setDomains] = useState<string[]>([]);
  const [domainDraft, setDomainDraft] = useState("");
  const [bindings, setBindings] = useState<string[]>([]);
  const [compatibilityFlags, setCompatibilityFlags] = useState<string[]>([]);
  const [flagDraft, setFlagDraft] = useState("");
  const [cpuMs, setCpuMs] = useState("");
  const [subrequests, setSubrequests] = useState("");
  const [diskMib, setDiskMib] = useState("");
  const [memoryMib, setMemoryMib] = useState("");
  const [cpuRatePercent, setCpuRatePercent] = useState("");
  const [maxProcesses, setMaxProcesses] = useState("");

  useEffect(() => {
    setNetworkMode(existing?.networkMode || request.networkMode || "deny");
    setDomains(initialList(request.domains, existing?.domains));
    setBindings(initialList(request.bindings, existing?.bindings));
    setCompatibilityFlags(
      initialList(request.compatibilityFlags, existing?.compatibilityFlags),
    );
    setCpuMs(String(existing?.cpuMs ?? request.cpuMs ?? limits.cpu_ms ?? ""));
    setSubrequests(
      String(existing?.subrequests ?? request.subrequests ?? limits.subrequests ?? ""),
    );
    setDiskMib(
      String(existing?.diskMib ?? request.diskMib ?? limits.disk_mib ?? ""),
    );
    setMemoryMib(
      String(existing?.memoryMib ?? request.memoryMib ?? limits.memory_mib ?? ""),
    );
    setCpuRatePercent(
      String(
        existing?.cpuRatePercent ??
          request.cpuRatePercent ??
          limits.cpu_rate_percent ??
          "",
      ),
    );
    setMaxProcesses(
      String(
        existing?.maxProcesses ?? request.maxProcesses ?? limits.max_processes ?? "",
      ),
    );
    setDomainDraft("");
    setFlagDraft("");
  }, [consent, existing, request, limits]);

  const selectedDomains = useMemo(() => uniqueValues(domains), [domains]);
  const suggestedDomains = useMemo(
    () => uniqueValues([...request.domains, ...selectedDomains]),
    [request.domains, selectedDomains],
  );

  function toggleValue(
    value: string,
    checked: boolean,
    setter: (next: string[]) => void,
    current: string[],
  ) {
    setter(
      checked
        ? uniqueValues([...current, value])
        : current.filter((item) => item.toLowerCase() !== value.toLowerCase()),
    );
  }

  function addDomain() {
    const value = domainDraft.trim();
    if (!value) return;
    setDomains((current) => uniqueValues([...current, value]));
    setDomainDraft("");
  }

  function addFlag() {
    const value = flagDraft.trim();
    if (!value) return;
    setCompatibilityFlags((current) => uniqueValues([...current, value]));
    setFlagDraft("");
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
                Review and adjust the grant before enabling. You may add or remove
                capabilities relative to the plugin request; host hard caps still apply.
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
              <Badge className="bg-ink/5 text-ink/60 normal-case tracking-normal">
                {isWorkerd ? "workerd" : "native"}
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
            <p className="rounded-md border border-ink/10 bg-fold/40 px-3 py-2 text-sm text-ink/70">
              Overrides that remove capabilities the guest needs may break the plugin.
              Bookclerk enforces the grant you approve; it does not guarantee guest
              behaviour afterward.
            </p>
          </section>

          <section className="space-y-3">
            <div>
              <h3 className="text-sm font-semibold text-ink">Network</h3>
              <p className="text-sm text-ink/55">
                {isWorkerd
                  ? "Workerd guests enforce deny/outbound plus the domain allowlist below."
                  : "Native guests use OS-jail allow-or-deny for network (no hostname filter), plus the same jail memory/CPU/process and disk budgets as workerd."}
              </p>
            </div>
            <select
              className="w-full rounded-md border border-ink/15 bg-white/80 px-3 py-2 text-sm text-ink shadow-sm focus:border-teal focus:outline-none focus:ring-2 focus:ring-teal/30"
              value={networkMode}
              disabled={busy}
              onChange={(e) => setNetworkMode(e.target.value)}
            >
              <option value="outbound">Allow outbound network</option>
              <option value="deny">Deny network</option>
            </select>
          </section>

          {isWorkerd ? (
            <section className="space-y-3">
              <div>
                <h3 className="text-sm font-semibold text-ink">Allowed domains</h3>
                <p className="text-sm text-ink/55">
                  Add or remove initial hosts. Suggested values come from the plugin
                  request; extra domains are allowed.
                </p>
              </div>
              <div className="flex flex-wrap gap-2">
                {suggestedDomains.map((domain) => {
                  const checked = selectedDomains.some(
                    (item) => item.toLowerCase() === domain.toLowerCase(),
                  );
                  const extra = !request.domains.some(
                    (item) => item.toLowerCase() === domain.toLowerCase(),
                  );
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
                            ? current.filter(
                                (item) => item.toLowerCase() !== domain.toLowerCase(),
                              )
                            : uniqueValues([...current, domain]),
                        )
                      }
                    >
                      {domain}
                      {extra ? " +" : ""}
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
                  placeholder="Add domain (e.g. api.example.com)"
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

          <section className="space-y-3">
            <div>
              <h3 className="text-sm font-semibold text-ink">Host bindings</h3>
              <p className="text-sm text-ink/55">
                Shared by native and workerd guests. You may grant bindings the
                plugin did not request, or remove requested ones.
              </p>
            </div>
            <div className="grid gap-2 sm:grid-cols-2">
              {knownBindings.map((binding) => {
                const requested = request.bindings.some(
                  (item) => item.toLowerCase() === binding.toLowerCase(),
                );
                return (
                  <label key={binding} className="flex items-center gap-2 text-sm text-ink">
                    <input
                      type="checkbox"
                      className="h-4 w-4 accent-teal"
                      checked={bindings.some(
                        (item) => item.toLowerCase() === binding.toLowerCase(),
                      )}
                      disabled={busy}
                      onChange={(e) =>
                        toggleValue(binding, e.target.checked, setBindings, bindings)
                      }
                    />
                    <span>
                      {binding}
                      {!requested ? (
                        <span className="ml-1 text-xs text-ink/45">extra</span>
                      ) : null}
                    </span>
                  </label>
                );
              })}
            </div>
          </section>

          <section className="space-y-3">
            <div>
              <h3 className="text-sm font-semibold text-ink">Jail resources</h3>
              <p className="text-sm text-ink/55">
                {isWorkerd
                  ? "Memory, processes, and disk apply via the OS jail. Workerd CPU is set below as isolate cpu_ms; jail CPU rate comes from the host default / Settings ceiling (per-jail, not a shared pool)."
                  : "Applied via the OS jail (cgroup / Job Object). CPU rate is percent of one logical CPU (100 = one core; above 100 uses multiple cores). Disk budgets cover each of data/ and tmp/."}
              </p>
            </div>
            <div className="grid gap-3 sm:grid-cols-2">
              <label className="space-y-1.5 text-sm font-medium text-ink">
                Memory MiB
                <Input
                  type="number"
                  min={1}
                  max={limits.max_memory_mib}
                  value={memoryMib}
                  disabled={busy}
                  onChange={(e) => setMemoryMib(e.target.value)}
                />
                <span className="block text-xs font-normal text-ink/50">
                  Max {limits.max_memory_mib}
                </span>
              </label>
              {!isWorkerd ? (
                <label className="space-y-1.5 text-sm font-medium text-ink">
                  CPU rate % (of one core)
                  <Input
                    type="number"
                    min={1}
                    max={limits.max_cpu_rate_percent}
                    value={cpuRatePercent}
                    disabled={busy}
                    onChange={(e) => setCpuRatePercent(e.target.value)}
                  />
                  <span className="block text-xs font-normal text-ink/50">
                    Max {limits.max_cpu_rate_percent} (host cores × 100). This is a
                    per-jail ceiling; Settings may lower it further. Oversubscribed
                    plugins contend via the OS scheduler (quotas are not a shared
                    reservation pool).
                  </span>
                </label>
              ) : null}
              <label className="space-y-1.5 text-sm font-medium text-ink">
                Max processes
                <Input
                  type="number"
                  min={1}
                  max={limits.max_max_processes}
                  value={maxProcesses}
                  disabled={busy}
                  onChange={(e) => setMaxProcesses(e.target.value)}
                />
                <span className="block text-xs font-normal text-ink/50">
                  Max {limits.max_max_processes}
                </span>
              </label>
              <label className="space-y-1.5 text-sm font-medium text-ink">
                Disk MiB
                <Input
                  type="number"
                  min={1}
                  max={limits.max_disk_mib}
                  value={diskMib}
                  disabled={busy}
                  onChange={(e) => setDiskMib(e.target.value)}
                />
                <span className="block text-xs font-normal text-ink/50">
                  Max {limits.max_disk_mib} (each for data/ and tmp/)
                </span>
              </label>
            </div>
          </section>

          {isWorkerd ? (
            <section className="space-y-3">
              <div>
                <h3 className="text-sm font-semibold text-ink">Workerd isolate limits</h3>
                <p className="text-sm text-ink/55">
                  Isolate CPU soft budget and egress subrequest budget. Jail CPU
                  rate is not per-plugin for workerd — it comes from the host
                  default / Settings per-jail ceiling. Host maxes:{" "}
                  {limits.max_cpu_ms} ms / {limits.max_subrequests} subrequests.
                </p>
              </div>
              <div className="grid gap-3 sm:grid-cols-2">
                <label className="space-y-1.5 text-sm font-medium text-ink">
                  CPU milliseconds
                  <Input
                    type="number"
                    min={1}
                    max={limits.max_cpu_ms}
                    value={cpuMs}
                    disabled={busy}
                    onChange={(e) => setCpuMs(e.target.value)}
                  />
                </label>
                <label className="space-y-1.5 text-sm font-medium text-ink">
                  Subrequests
                  <Input
                    type="number"
                    min={1}
                    max={limits.max_subrequests}
                    value={subrequests}
                    disabled={busy}
                    onChange={(e) => setSubrequests(e.target.value)}
                  />
                </label>
              </div>
            </section>
          ) : null}

          {isWorkerd ? (
            <section className="space-y-3">
              <div>
                <h3 className="text-sm font-semibold text-ink">Compatibility flags</h3>
                <p className="text-sm text-ink/55">
                  Workerd-only. Add or remove flags relative to the plugin request.
                </p>
              </div>
              <div className="grid gap-2 sm:grid-cols-2">
                {uniqueValues([
                  ...request.compatibilityFlags,
                  ...compatibilityFlags,
                ]).map((flag) => (
                  <label key={flag} className="flex items-center gap-2 text-sm text-ink">
                    <input
                      type="checkbox"
                      className="h-4 w-4 accent-teal"
                      checked={compatibilityFlags.some(
                        (item) => item.toLowerCase() === flag.toLowerCase(),
                      )}
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
              <div className="flex gap-2">
                <Input
                  value={flagDraft}
                  disabled={busy}
                  onChange={(e) => setFlagDraft(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      addFlag();
                    }
                  }}
                  placeholder="Add compatibility flag"
                  autoComplete="off"
                  spellCheck={false}
                />
                <Button
                  type="button"
                  variant="secondary"
                  disabled={busy || !flagDraft.trim()}
                  onClick={addFlag}
                >
                  Add
                </Button>
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
                compatibilityFlags: isWorkerd ? compatibilityFlags : [],
                cpuMs: isWorkerd
                  ? parsePositiveInt(cpuMs, limits.max_cpu_ms)
                  : undefined,
                subrequests: isWorkerd
                  ? parsePositiveInt(subrequests, limits.max_subrequests)
                  : undefined,
                diskMib: parsePositiveInt(diskMib, limits.max_disk_mib),
                memoryMib: parsePositiveInt(memoryMib, limits.max_memory_mib),
                cpuRatePercent: isWorkerd
                  ? undefined
                  : parsePositiveInt(cpuRatePercent, limits.max_cpu_rate_percent),
                maxProcesses: parsePositiveInt(maxProcesses, limits.max_max_processes),
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
