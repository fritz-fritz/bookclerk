import { useEffect, useState } from "react";
import { DiscoverPage } from "@/components/DiscoverPage";
import { LibraryPage } from "@/components/LibraryPage";
import { LoginPage } from "@/components/LoginPage";
import { authMe } from "@/lib/api";

type AuthState = "loading" | "anon" | "authed";
type View = "library" | "discover";

export default function App() {
  const [auth, setAuth] = useState<AuthState>("loading");
  const [view, setView] = useState<View>("library");

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const ok = await authMe();
        if (!cancelled) setAuth(ok ? "authed" : "anon");
      } catch {
        if (!cancelled) setAuth("anon");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  if (auth === "loading") {
    return (
      <div className="flex min-h-full items-center justify-center text-sm text-ink/60">
        Loading Bookclerk…
      </div>
    );
  }

  if (auth === "anon") {
    return <LoginPage onSuccess={() => setAuth("authed")} />;
  }

  if (view === "discover") {
    return (
      <DiscoverPage
        onLogout={() => setAuth("anon")}
        onShowLibrary={() => setView("library")}
      />
    );
  }

  return (
    <LibraryPageWithNav
      onLogout={() => setAuth("anon")}
      onShowDiscover={() => setView("discover")}
    />
  );
}

function LibraryPageWithNav({
  onLogout,
  onShowDiscover,
}: {
  onLogout: () => void;
  onShowDiscover: () => void;
}) {
  return (
    <div className="relative h-full">
      <div className="pointer-events-none absolute left-24 top-4 z-20 sm:left-32">
        <button
          type="button"
          className="pointer-events-auto text-sm text-ink/60 hover:text-ink"
          onClick={onShowDiscover}
        >
          Discover
        </button>
      </div>
      <LibraryPage onLogout={onLogout} />
    </div>
  );
}
