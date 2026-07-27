import { useEffect, useState } from "react";
import { LogOut, Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  createRequest,
  fetchDiscoverFeed,
  fetchRequests,
  logout,
  patchRequest,
  type DiscoverFeed,
  type Recommendation,
  type TitleRequest,
} from "@/lib/api";

export function DiscoverPage({
  onLogout,
  onShowLibrary,
}: {
  onLogout: () => void;
  onShowLibrary: () => void;
}) {
  const [feed, setFeed] = useState<DiscoverFeed>({ shelves: [] });
  const [requests, setRequests] = useState<TitleRequest[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [title, setTitle] = useState("");
  const [authors, setAuthors] = useState("");

  async function refresh() {
    setError(null);
    setBusy(true);
    try {
      const [f, q] = await Promise.all([fetchDiscoverFeed(12), fetchRequests()]);
      setFeed(f);
      setRequests(q);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load discovery");
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    void refresh();
  }, []);

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
    await logout();
    onLogout();
  }

  return (
    <div className="flex h-full flex-col">
      <header className="sticky top-0 z-10 border-b border-ink/10 bg-paper/85 px-3 py-3 backdrop-blur-md sm:px-5">
        <div className="mx-auto flex max-w-6xl items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            <img
              src="/bookclerk-logo.svg"
              alt="Bookclerk"
              className="h-8 w-auto sm:h-9"
            />
            <nav className="flex gap-2 text-sm">
              <button
                type="button"
                className="text-ink/60 hover:text-ink"
                onClick={onShowLibrary}
              >
                Library
              </button>
              <span className="font-medium text-ink">Discover</span>
            </nav>
          </div>
          <div className="flex items-center gap-2">
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
          <h1 className="text-2xl font-semibold tracking-tight text-ink">Discover</h1>
          <p className="text-sm text-ink/60">
            Personalized shelves from your library, listening, and storefront catalogs.
          </p>
        </div>

        {feed.shelves.length === 0 ? (
          <p className="text-sm text-ink/50">No recommendations yet — finish or rate a few titles.</p>
        ) : (
          feed.shelves.map((shelf) => (
            <section key={shelf.id} className="space-y-3">
              <div>
                <h2 className="text-lg font-semibold text-ink">{shelf.title}</h2>
                {shelf.subtitle ? (
                  <p className="text-sm text-ink/55">{shelf.subtitle}</p>
                ) : null}
              </div>
              <div className="-mx-4 flex gap-3 overflow-x-auto px-4 pb-2 snap-x">
                {shelf.items.map((r, i) => (
                  <ShelfCard key={`${shelf.id}-${r.asin ?? r.isbn ?? r.title}-${i}`} rec={r} />
                ))}
              </div>
            </section>
          ))
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
                  <div className="flex gap-1">
                    {r.status === "open" ? (
                      <>
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
                      </>
                    ) : null}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </section>
      </main>
    </div>
  );
}

function ShelfCard({ rec }: { rec: Recommendation }) {
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
      {rec.purchase_hints.length > 0 ? (
        <p className="mt-2 flex flex-wrap gap-x-2 gap-y-1 text-[11px] text-teal">
          {rec.purchase_hints.slice(0, 2).map((h) =>
            h.url ? (
              <a
                key={`${h.source}-${h.product_id}`}
                href={h.url}
                target="_blank"
                rel="noreferrer"
                className="underline"
              >
                {h.source}
              </a>
            ) : (
              <span key={`${h.source}-${h.product_id}`}>{h.source}</span>
            ),
          )}
        </p>
      ) : null}
    </article>
  );
}
