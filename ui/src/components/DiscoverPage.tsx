import { useEffect, useRef, useState } from "react";
import { LogOut, Settings2, Sparkles } from "lucide-react";
import { AppNav, type AppNavProps } from "@/components/AppNav";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  createRequest,
  fetchDiscoverFeed,
  fetchPreferences,
  fetchPurchaseHints,
  fetchRequests,
  patchPreferences,
  patchRequest,
  signOut,
  type AppView,
  type AuthRole,
  type DiscoverFeed,
  type DiscoverShelf,
  type PurchaseHint,
  type Recommendation,
  type ShelfKindInfo,
  type TitleRequest,
} from "@/lib/api";

const SHELF_CHUNK = 8;
const SHELVES_INITIAL = 6;

function shelfMatchesIgnore(shelfId: string, ignored: string[]): boolean {
  const id = shelfId.toLowerCase();
  for (const raw of ignored) {
    const d = raw.trim().toLowerCase();
    if (!d) continue;
    if (id === d) return true;
    if (id.startsWith(`${d}:`)) return true;
    if (d === "from_store" && id.startsWith("from_")) return true;
  }
  return false;
}

export function DiscoverPage({
  onLogout,
  nav,
  canModerateRequests,
  role,
  defaultView,
  onDefaultViewChange,
}: {
  onLogout: () => void;
  nav: AppNavProps;
  canModerateRequests: boolean;
  role?: AuthRole;
  defaultView: AppView;
  onDefaultViewChange?: (view: AppView) => void;
}) {
  const [feed, setFeed] = useState<DiscoverFeed>({ shelves: [] });
  const [requests, setRequests] = useState<TitleRequest[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [title, setTitle] = useState("");
  const [authors, setAuthors] = useState("");
  const [ignored, setIgnored] = useState<string[]>([]);
  const [prefsView, setPrefsView] = useState<AppView>(defaultView);
  const [prefsOpen, setPrefsOpen] = useState(false);
  const [prefsReady, setPrefsReady] = useState(false);
  const [visibleShelves, setVisibleShelvesCount] = useState(SHELVES_INITIAL);
  const shelfSentinelRef = useRef<HTMLDivElement | null>(null);

  async function refreshFeed() {
    const [f, q] = await Promise.all([fetchDiscoverFeed(36), fetchRequests()]);
    setFeed(f);
    setRequests(q);
    setVisibleShelvesCount(SHELVES_INITIAL);
  }

  async function refresh() {
    setError(null);
    setBusy(true);
    try {
      await refreshFeed();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load discovery");
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      setBusy(true);
      setError(null);
      try {
        const prefs = await fetchPreferences();
        if (cancelled) return;
        setIgnored(prefs.disabled_shelves);
        setPrefsView(prefs.default_view);
        await refreshFeed();
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : "Failed to load discovery");
        }
      } finally {
        if (!cancelled) {
          setPrefsReady(true);
          setBusy(false);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  async function toggleIgnored(kindId: string) {
    const prev = ignored;
    const next = prev.includes(kindId)
      ? prev.filter((x) => x !== kindId)
      : [...prev, kindId];
    setIgnored(next);
    setBusy(true);
    setError(null);
    try {
      const saved = await patchPreferences({ disabled_shelves: next });
      setIgnored(saved.disabled_shelves);
      await refreshFeed();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save shelf prefs");
      setIgnored(prev);
    } finally {
      setBusy(false);
    }
  }

  async function onDefaultViewSelect(view: AppView) {
    const prev = prefsView;
    setPrefsView(view);
    setBusy(true);
    setError(null);
    try {
      const saved = await patchPreferences({ default_view: view });
      setPrefsView(saved.default_view);
      onDefaultViewChange?.(saved.default_view);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save default view");
      setPrefsView(prev);
    } finally {
      setBusy(false);
    }
  }

  async function onAddRequest() {
    if (!title.trim()) return;
    setBusy(true);
    setError(null);
    try {
      await createRequest({
        title: title.trim(),
        authors: authors.trim() || undefined,
      });
      setTitle("");
      setAuthors("");
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create request");
      setBusy(false);
    }
  }

  async function onStatus(uuid: string, status: string) {
    setBusy(true);
    try {
      await patchRequest(uuid, { status });
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to update request");
      setBusy(false);
    }
  }

  async function onSignOut() {
    await signOut(role);
    onLogout();
  }

  const shelfKinds: ShelfKindInfo[] = feed.shelf_kinds?.length
    ? feed.shelf_kinds
    : [];
  const filteredShelves = feed.shelves.filter(
    (s) => !shelfMatchesIgnore(s.id, ignored),
  );
  const shownShelves = filteredShelves.slice(0, visibleShelves);

  useEffect(() => {
    const el = shelfSentinelRef.current;
    if (!el) return;
    const obs = new IntersectionObserver(
      (entries) => {
        if (!entries.some((e) => e.isIntersecting)) return;
        setVisibleShelvesCount((n) =>
          Math.min(n + SHELVES_INITIAL, filteredShelves.length),
        );
      },
      { rootMargin: "160px" },
    );
    obs.observe(el);
    return () => obs.disconnect();
  }, [filteredShelves.length, shownShelves.length]);

  return (
    <div className="flex h-full flex-col">
      <header className="sticky top-0 z-10 border-b border-ink/10 bg-paper/85 px-3 py-3 backdrop-blur-md sm:px-5">
        <div className="mx-auto flex max-w-6xl items-center justify-between gap-3">
          <div className="flex items-center gap-3 sm:gap-5">
            <img
              src="/bookclerk-logo.svg"
              alt="Bookclerk"
              className="h-8 w-auto sm:h-9"
            />
            <AppNav {...nav} />
          </div>
          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              onClick={() => setPrefsOpen((o) => !o)}
              aria-label="Preferences"
              aria-expanded={prefsOpen}
            >
              <Settings2 className="h-4 w-4" />
            </Button>
            <Button
              variant="secondary"
              onClick={() => void refresh()}
              disabled={busy}
            >
              <Sparkles className="h-4 w-4" />
              Refresh
            </Button>
            <Button variant="ghost" onClick={() => void onSignOut()} aria-label="Sign out">
              <LogOut className="h-4 w-4" />
            </Button>
          </div>
        </div>
      </header>

      <main className="mx-auto flex w-full max-w-6xl flex-1 flex-col gap-10 overflow-auto px-4 py-6">
        {error ? (
          <p className="text-sm font-medium text-brick" role="alert">
            {error}
          </p>
        ) : null}

        <div className="space-y-1">
          <h1 className="font-display text-2xl font-semibold tracking-tight text-ink">
            Discover
          </h1>
          <p className="text-sm text-ink/60">
            Personalized shelves from your library, listening, and storefront catalogs.
          </p>
        </div>

        {prefsOpen && prefsReady ? (
          <section className="space-y-5 border border-ink/10 bg-white/40 p-4">
            <div className="space-y-2">
              <h2 className="text-base font-semibold text-ink">Default view</h2>
              <p className="text-sm text-ink/55">
                Where this account opens after sign-in. Saved to your Bookclerk profile.
              </p>
              <div className="flex flex-wrap gap-2">
                {(
                  [
                    ["discover", "Discover"],
                    ["library", "Library"],
                    ["accounts", "Accounts"],
                  ] as const
                ).map(([id, label]) => (
                  <Button
                    key={id}
                    variant={prefsView === id ? "secondary" : "ghost"}
                    disabled={busy}
                    onClick={() => void onDefaultViewSelect(id)}
                  >
                    {label}
                  </Button>
                ))}
              </div>
            </div>

            {shelfKinds.length > 0 ? (
              <div className="space-y-3">
                <div>
                  <h2 className="text-base font-semibold text-ink">Shelves to show</h2>
                  <p className="text-sm text-ink/55">
                    All shelves are on by default. Uncheck any you want to hide for this
                    account.
                  </p>
                </div>
                <ul className="grid gap-2 sm:grid-cols-2">
                  {shelfKinds.map((kind) => {
                    const on = !ignored.includes(kind.id);
                    return (
                      <li key={kind.id}>
                        <label className="flex cursor-pointer items-start gap-2 text-sm text-ink">
                          <input
                            type="checkbox"
                            className="mt-0.5"
                            checked={on}
                            disabled={busy}
                            onChange={() => void toggleIgnored(kind.id)}
                          />
                          <span>
                            <span className="font-medium">{kind.label}</span>
                            <span className="block text-xs text-ink/45">{kind.id}</span>
                          </span>
                        </label>
                      </li>
                    );
                  })}
                </ul>
              </div>
            ) : null}
          </section>
        ) : null}

        {filteredShelves.length === 0 ? (
          <p className="text-sm text-ink/50">No recommendations yet — finish or rate a few titles.</p>
        ) : (
          <>
            {shownShelves.map((shelf) => (
              <ShelfSection key={shelf.id} shelf={shelf} />
            ))}
            {shownShelves.length < filteredShelves.length ? (
              <div ref={shelfSentinelRef} className="h-6" aria-hidden />
            ) : null}
          </>
        )}

        <section className="space-y-3 border-t border-ink/10 pt-8">
          <h2 className="text-lg font-semibold text-ink">Request queue</h2>
          <div className="flex flex-col gap-2 sm:flex-row">
            <Input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="Title to request"
              className="sm:flex-1"
            />
            <Input
              value={authors}
              onChange={(e) => setAuthors(e.target.value)}
              placeholder="Authors (optional)"
              className="sm:w-64"
            />
            <Button onClick={() => void onAddRequest()} disabled={busy || !title.trim()}>
              Add request
            </Button>
          </div>
          {requests.length === 0 ? (
            <p className="text-sm text-ink/50">No requests yet.</p>
          ) : (
            <ul className="divide-y divide-ink/10 bg-white/35">
              {requests.map((r) => (
                <li
                  key={r.uuid}
                  className="flex flex-wrap items-center justify-between gap-2 px-3 py-3"
                >
                  <div>
                    <p className="font-medium text-ink">{r.title}</p>
                    <p className="text-xs text-ink/50">
                      {r.status} · {r.authors ?? "?"} · {r.uuid.slice(0, 8)}
                    </p>
                  </div>
                  {canModerateRequests && r.status === "open" ? (
                    <div className="flex gap-1">
                      <Button
                        variant="secondary"
                        onClick={() => void onStatus(r.uuid, "approved")}
                        disabled={busy}
                      >
                        Approve
                      </Button>
                      <Button
                        variant="ghost"
                        onClick={() => void onStatus(r.uuid, "cancelled")}
                        disabled={busy}
                      >
                        Cancel
                      </Button>
                    </div>
                  ) : null}
                </li>
              ))}
            </ul>
          )}
        </section>
      </main>
    </div>
  );
}

function ShelfSection({ shelf }: { shelf: DiscoverShelf }) {
  const [visible, setVisible] = useState(SHELF_CHUNK);
  const scrollerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    setVisible(SHELF_CHUNK);
  }, [shelf.id, shelf.items.length]);

  useEffect(() => {
    const el = scrollerRef.current;
    if (!el) return;
    const onScroll = () => {
      if (el.scrollLeft + el.clientWidth >= el.scrollWidth - 96) {
        setVisible((n) => Math.min(n + SHELF_CHUNK, shelf.items.length));
      }
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  }, [shelf.items.length]);

  const items = shelf.items.slice(0, visible);

  return (
    <section className="space-y-3">
      <div>
        <h2 className="text-lg font-semibold text-ink">{shelf.title}</h2>
        {shelf.subtitle ? (
          <p className="text-sm text-ink/55">{shelf.subtitle}</p>
        ) : null}
      </div>
      <div
        ref={scrollerRef}
        className="-mx-4 flex gap-3 overflow-x-auto px-4 pb-2 snap-x"
      >
        {items.map((r, i) => (
          <ShelfCard key={`${shelf.id}-${r.asin ?? r.isbn ?? r.title}-${i}`} rec={r} />
        ))}
        {visible < shelf.items.length ? (
          <div className="flex w-8 shrink-0 items-center justify-center text-xs text-ink/35">
            …
          </div>
        ) : null}
      </div>
    </section>
  );
}

function ShelfCard({ rec }: { rec: Recommendation }) {
  const [hints, setHints] = useState<PurchaseHint[]>(rec.purchase_hints);
  const [best, setBest] = useState<PurchaseHint | null>(
    rec.purchase_hints[0] ?? null,
  );
  const [pricing, setPricing] = useState<"idle" | "loading" | "done">("idle");

  useEffect(() => {
    let cancelled = false;
    setHints(rec.purchase_hints);
    setBest(rec.purchase_hints[0] ?? null);
    setPricing("loading");
    void (async () => {
      try {
        const res = await fetchPurchaseHints({
          title: rec.title,
          authors: rec.authors,
          asin: rec.asin,
          isbn: rec.isbn,
          candidate_source: rec.candidate_source,
          candidate_product_id: rec.candidate_product_id,
        });
        if (cancelled) return;
        setHints(res.hints);
        setBest(res.best);
      } catch {
        // Keep seed links from the feed.
      } finally {
        if (!cancelled) setPricing("done");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [
    rec.title,
    rec.authors,
    rec.asin,
    rec.isbn,
    rec.candidate_source,
    rec.candidate_product_id,
  ]);

  const others = hints.filter(
    (h) =>
      !(
        best &&
        h.source === best.source &&
        h.product_id === best.product_id
      ),
  );

  return (
    <article className="w-56 shrink-0 snap-start rounded-lg bg-white/50 p-3 shadow-sm ring-1 ring-ink/5">
      <p className="line-clamp-2 text-sm font-medium text-ink">{rec.title}</p>
      <p className="mt-1 line-clamp-1 text-xs text-ink/55">
        {rec.authors ?? "Unknown author"}
        {rec.series ? ` · ${rec.series}` : ""}
      </p>
      {rec.reasons[0] ? (
        <p className="mt-2 line-clamp-2 text-[11px] leading-snug text-ink/45">{rec.reasons[0]}</p>
      ) : null}

      {best ? (
        <div className="mt-2 space-y-1">
          {best.url ? (
            <a
              href={best.url}
              target="_blank"
              rel="noreferrer"
              className="inline-flex max-w-full items-baseline gap-1 text-[12px] font-semibold text-teal underline"
            >
              <span className="truncate capitalize">{storeLabel(best.source)}</span>
              <span className="shrink-0 tabular-nums">
                {best.price_label ?? (pricing === "loading" ? "…" : "")}
              </span>
            </a>
          ) : (
            <p className="text-[12px] font-semibold capitalize text-ink/70">
              {storeLabel(best.source)}
              {best.price_label ? ` · ${best.price_label}` : ""}
            </p>
          )}
          {others.length > 0 ? (
            <p className="flex flex-wrap gap-x-2 gap-y-1 text-[11px] text-ink/55">
              {others.map((h) =>
                h.url ? (
                  <a
                    key={`${h.source}-${h.product_id}`}
                    href={h.url}
                    target="_blank"
                    rel="noreferrer"
                    className="underline hover:text-teal"
                  >
                    {storeLabel(h.source)}
                    {h.price_label ? ` ${h.price_label}` : ""}
                  </a>
                ) : (
                  <span key={`${h.source}-${h.product_id}`}>
                    {storeLabel(h.source)}
                  </span>
                ),
              )}
            </p>
          ) : null}
        </div>
      ) : null}
    </article>
  );
}

function storeLabel(source: string): string {
  switch (source.toLowerCase()) {
    case "audible":
      return "Audible";
    case "libro":
      return "Libro.fm";
    case "chirp":
      return "Chirp";
    case "graphicaudio":
      return "GraphicAudio";
    default:
      return source;
  }
}
