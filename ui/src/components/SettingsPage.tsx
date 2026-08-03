import { useEffect, useState, type FormEvent } from "react";
import { LogOut, RefreshCw } from "lucide-react";
import { AppNav, type AppNavProps } from "@/components/AppNav";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  fetchPreferences,
  fetchSettings,
  patchPreferences,
  patchSettings,
  signOut,
  type AuthRole,
  type SettingsResponse,
  type UserPreferences,
} from "@/lib/api";

export function SettingsPage({
  onLogout,
  nav,
  role,
}: {
  onLogout: () => void;
  nav: AppNavProps;
  role?: AuthRole;
}) {
  const [preferences, setPreferences] = useState<UserPreferences | null>(null);
  const [settings, setSettings] = useState<SettingsResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [daemonListen, setDaemonListen] = useState("");
  const [daemonAuthEnabled, setDaemonAuthEnabled] = useState(true);
  const [autoAcquire, setAutoAcquire] = useState(false);

  async function refresh() {
    setError(null);
    setLoading(true);
    try {
      const [prefs, nextSettings] = await Promise.all([fetchPreferences(), fetchSettings()]);
      setPreferences(prefs);
      setSettings(nextSettings);
      setDaemonListen(nextSettings.settings["daemon.listen"] ?? "");
      setDaemonAuthEnabled(nextSettings.settings["daemon.auth.enabled"] === "true");
      setAutoAcquire(nextSettings.settings["library.auto_acquire"] === "true");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load settings");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void refresh();
  }, []);

  async function onPreferencesSave(e: FormEvent) {
    e.preventDefault();
    if (!preferences) return;
    setSaving(true);
    setError(null);
    try {
      const next = await patchPreferences({
        default_view: preferences.default_view,
        disabled_shelves: preferences.disabled_shelves,
      });
      setPreferences(next);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save preferences");
    } finally {
      setSaving(false);
    }
  }

  async function onOperatorSave(e: FormEvent) {
    e.preventDefault();
    setSaving(true);
    setError(null);
    try {
      const next = await patchSettings({
        settings: [
          { key: "daemon.listen", value: daemonListen },
          { key: "daemon.auth.enabled", value: String(daemonAuthEnabled) },
          { key: "library.auto_acquire", value: String(autoAcquire) },
        ],
      });
      setSettings(next);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to save operator settings");
    } finally {
      setSaving(false);
    }
  }

  async function onSignOut() {
    await signOut(role);
    onLogout();
  }

  return (
    <div className="flex h-full flex-col">
      <header className="sticky top-0 z-10 border-b border-ink/10 bg-paper/85 px-3 py-3 backdrop-blur-md sm:px-5">
        <div className="mx-auto flex max-w-6xl items-center justify-between gap-3">
          <div className="flex items-center gap-3 sm:gap-5">
            <img src="/bookclerk-logo.svg" alt="Bookclerk" className="h-8 w-auto sm:h-9" />
            <AppNav {...nav} />
          </div>
          <div className="flex items-center gap-2">
            <Button variant="secondary" onClick={() => void refresh()} disabled={loading || saving}>
              <RefreshCw className="h-4 w-4" />
              Refresh
            </Button>
            <Button variant="ghost" onClick={() => void onSignOut()} aria-label="Sign out">
              <LogOut className="h-4 w-4" />
            </Button>
          </div>
        </div>
      </header>

      <main className="mx-auto flex w-full max-w-6xl flex-1 flex-col gap-8 overflow-auto px-4 py-6">
        <div className="space-y-1">
          <h1 className="font-display text-2xl font-semibold tracking-tight text-ink">Settings</h1>
          <p className="text-sm text-ink/60">Manage your preferences and operator configuration.</p>
        </div>

        {error ? <p className="text-sm font-medium text-brick" role="alert">{error}</p> : null}

        <section className="space-y-3 rounded-lg border border-ink/10 bg-white/40 p-4">
          <h2 className="text-lg font-semibold text-ink">Preferences</h2>
          {preferences ? (
            <form className="grid gap-3 sm:grid-cols-2" onSubmit={(e) => void onPreferencesSave(e)}>
              <label className="space-y-1 text-sm text-ink/70">
                <span>Default view</span>
                <select
                  className="w-full rounded-md border border-ink/10 bg-paper px-3 py-2"
                  value={preferences.default_view}
                  onChange={(e) =>
                    setPreferences((current) => current ? { ...current, default_view: e.target.value as UserPreferences["default_view"] } : current)
                  }
                >
                  <option value="discover">Discover</option>
                  <option value="library">Library</option>
                  <option value="wishlist">Wishlist</option>
                  <option value="accounts">Accounts</option>
                </select>
              </label>
              <label className="space-y-1 text-sm text-ink/70">
                <span>Disabled shelves</span>
                <Input
                  value={preferences.disabled_shelves.join(",")}
                  onChange={(e) =>
                    setPreferences((current) => current ? { ...current, disabled_shelves: e.target.value.split(",").map((item) => item.trim()).filter(Boolean) } : current)
                  }
                  placeholder="shelf-a,shelf-b"
                />
              </label>
              <div className="sm:col-span-2">
                <Button type="submit" disabled={saving}>
                  Save preferences
                </Button>
              </div>
            </form>
          ) : (
            <p className="text-sm text-ink/50">Loading preferences…</p>
          )}
        </section>

        {role === "operator" ? (
          <section className="space-y-3 rounded-lg border border-ink/10 bg-white/40 p-4">
            <h2 className="text-lg font-semibold text-ink">Operator configuration</h2>
            {settings ? (
              <form className="grid gap-3" onSubmit={(e) => void onOperatorSave(e)}>
                <label className="space-y-1 text-sm text-ink/70">
                  <span>Daemon listen address</span>
                  <Input value={daemonListen} onChange={(e) => setDaemonListen(e.target.value)} />
                </label>
                <label className="flex items-center gap-2 text-sm text-ink/70">
                  <input
                    type="checkbox"
                    checked={daemonAuthEnabled}
                    onChange={(e) => setDaemonAuthEnabled(e.target.checked)}
                  />
                  Enable daemon auth
                </label>
                <label className="flex items-center gap-2 text-sm text-ink/70">
                  <input
                    type="checkbox"
                    checked={autoAcquire}
                    onChange={(e) => setAutoAcquire(e.target.checked)}
                  />
                  Auto acquire
                </label>
                <div className="space-y-2">
                  <p className="text-sm font-medium text-ink">Plugin settings</p>
                  {settings.plugins.map((plugin) => (
                    <div key={plugin.id} className="rounded-md border border-ink/10 bg-paper/50 p-3">
                      <p className="text-sm font-semibold text-ink">{plugin.id}</p>
                      {plugin.settings.map((option) => (
                        <label key={`${plugin.id}-${option.key}`} className="mt-2 flex items-center gap-2 text-sm text-ink/70">
                          <span>{option.label}</span>
                          <Input value={option.value} readOnly />
                        </label>
                      ))}
                    </div>
                  ))}
                </div>
                <div>
                  <Button type="submit" disabled={saving}>
                    Save operator settings
                  </Button>
                </div>
              </form>
            ) : (
              <p className="text-sm text-ink/50">Loading operator settings…</p>
            )}
          </section>
        ) : null}
      </main>
    </div>
  );
}
