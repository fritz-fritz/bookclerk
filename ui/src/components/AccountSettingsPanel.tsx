import { useState, type FormEvent } from "react";
import { KeyRound, ShieldAlert, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  deleteUser,
  elevate,
  endElevate,
  isApiError,
  setPassword,
  type AuthSession,
} from "@/lib/api";

/**
 * Self-service account security: password, passkeys placeholder, delete, elevate.
 */
export function AccountSettingsPanel({
  session,
  onSessionChange,
  onDeleted,
}: {
  session: AuthSession | null;
  onSessionChange?: () => void | Promise<void>;
  onDeleted?: () => void | Promise<void>;
}) {
  const user = session?.user;
  const isOperatorOnly = session?.role === "operator" && !user;
  const [password, setPasswordValue] = useState("");
  const [passwordConfirm, setPasswordConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [elevateToken, setElevateToken] = useState("");
  const [elevateBusy, setElevateBusy] = useState(false);
  const [deleteConfirm, setDeleteConfirm] = useState("");

  async function onChangePassword(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setNotice(null);
    if (password.length < 8) {
      setError("Password must be at least 8 characters.");
      return;
    }
    if (password !== passwordConfirm) {
      setError("Passwords do not match.");
      return;
    }
    setBusy(true);
    try {
      const res = await setPassword({ password });
      setPasswordValue("");
      setPasswordConfirm("");
      setNotice(
        res.revoked_sessions > 0
          ? `Password updated. ${res.revoked_sessions} other session(s) were signed out.`
          : "Password updated.",
      );
      await onSessionChange?.();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to update password");
    } finally {
      setBusy(false);
    }
  }

  async function onDeleteAccount() {
    if (!user) return;
    if (deleteConfirm.trim().toLowerCase() !== "delete") {
      setError('Type "delete" to confirm account deletion.');
      return;
    }
    if (
      !window.confirm(
        "Permanently delete your Bookclerk account? Acquired books stay on the host; your wishlist, sessions, and store links are removed.",
      )
    ) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await deleteUser(user.id);
      await onDeleted?.();
    } catch (err) {
      if (isApiError(err) && err.status === 409) {
        setError("Cannot delete the last active administrator.");
      } else {
        setError(err instanceof Error ? err.message : "Failed to delete account");
      }
      setBusy(false);
    }
  }

  async function onElevate(e: FormEvent) {
    e.preventDefault();
    setElevateBusy(true);
    setError(null);
    try {
      await elevate(elevateToken.trim());
      setElevateToken("");
      await onSessionChange?.();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Elevation failed");
    } finally {
      setElevateBusy(false);
    }
  }

  async function onEndElevate() {
    setElevateBusy(true);
    setError(null);
    try {
      await endElevate();
      await onSessionChange?.();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to end elevation");
    } finally {
      setElevateBusy(false);
    }
  }

  return (
    <div className="flex flex-col gap-10">
      {error ? (
        <p className="text-sm font-medium text-brick" role="alert">
          {error}
        </p>
      ) : null}
      {notice ? (
        <p className="text-sm font-medium text-teal" role="status">
          {notice}
        </p>
      ) : null}

      <section className="space-y-3">
        <div className="space-y-1">
          <h2 className="text-lg font-semibold text-ink">Profile</h2>
          <p className="text-sm text-ink/55">
            How this session identifies you on this Bookclerk host.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-4 bg-white/35 px-4 py-4">
          <div className="relative flex h-14 w-14 items-center justify-center rounded-full bg-ink/10 text-lg font-semibold text-ink">
            {(user?.display_name || session?.role || "?")
              .trim()
              .charAt(0)
              .toUpperCase()}
            {session?.authenticated ? (
              <span
                className="absolute bottom-0.5 right-0.5 h-3 w-3 rounded-full border-2 border-paper bg-teal"
                title="Signed in"
              />
            ) : null}
          </div>
          <div className="min-w-0 space-y-1">
            <div className="flex flex-wrap items-center gap-2">
              <p className="font-semibold text-ink">
                {user?.display_name?.trim() ||
                  (isOperatorOnly ? "Operator" : "Account")}
              </p>
              <Badge className="bg-ink/10 text-ink normal-case tracking-normal">
                {session?.role ?? "unknown"}
              </Badge>
            </div>
            <p className="text-sm text-ink/55">
              {user
                ? `User #${user.id}`
                : isOperatorOnly
                  ? "Daemon operator token session"
                  : "No first-party user on this session"}
            </p>
          </div>
        </div>
      </section>

      {user ? (
        <section className="space-y-3">
          <div className="space-y-1">
            <h2 className="text-lg font-semibold text-ink">Password</h2>
            <p className="text-sm text-ink/55">
              Changing your password signs out other devices. Passkeys are not
              available yet.
            </p>
          </div>
          <form
            className="grid max-w-xl gap-3 bg-white/35 px-4 py-4 sm:grid-cols-2"
            onSubmit={(e) => void onChangePassword(e)}
          >
            <Input
              type="password"
              autoComplete="new-password"
              placeholder="New password"
              aria-label="New password"
              value={password}
              onChange={(e) => setPasswordValue(e.target.value)}
              disabled={busy}
            />
            <Input
              type="password"
              autoComplete="new-password"
              placeholder="Confirm password"
              aria-label="Confirm password"
              value={passwordConfirm}
              onChange={(e) => setPasswordConfirm(e.target.value)}
              disabled={busy}
            />
            <div className="sm:col-span-2">
              <Button type="submit" disabled={busy || !password}>
                <KeyRound className="h-4 w-4" />
                {busy ? "Saving…" : "Update password"}
              </Button>
            </div>
          </form>
          <div className="flex items-start gap-3 border border-dashed border-ink/15 bg-white/20 px-4 py-3 text-sm text-ink/55">
            <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
            <div>
              <p className="font-medium text-ink/70">Passkeys</p>
              <p>WebAuthn / passkey sign-in is planned; password remains the supported method.</p>
            </div>
          </div>
        </section>
      ) : (
        <section className="space-y-2 bg-white/35 px-4 py-4 text-sm text-ink/60">
          <p className="font-medium text-ink">Password &amp; passkeys</p>
          <p>
            Operator-only sessions use the daemon token. Create or sign in as an
            administrator or member to manage a local password.
          </p>
        </section>
      )}

      {session?.role === "administrator" && !session.impersonating ? (
        <section className="space-y-3">
          <div className="space-y-1">
            <h2 className="text-lg font-semibold text-ink">Operator elevation</h2>
            <p className="text-sm text-ink/55">
              Re-enter the operator token to unlock Server Settings and Plugins
              for this session.
            </p>
          </div>
          {session.elevated ? (
            <div className="flex flex-wrap items-center justify-between gap-3 bg-teal/10 px-4 py-3">
              <p className="text-sm text-ink">Elevation active for this session.</p>
              <Button
                type="button"
                variant="secondary"
                disabled={elevateBusy}
                onClick={() => void onEndElevate()}
              >
                {elevateBusy ? "Ending…" : "End elevation"}
              </Button>
            </div>
          ) : (
            <form
              className="flex flex-wrap gap-2 bg-white/35 px-4 py-3"
              onSubmit={(e) => void onElevate(e)}
            >
              <Input
                className="min-w-64 flex-1"
                type="password"
                value={elevateToken}
                onChange={(e) => setElevateToken(e.target.value)}
                placeholder="Operator token"
                autoComplete="off"
              />
              <Button type="submit" disabled={elevateBusy || !elevateToken.trim()}>
                {elevateBusy ? "Elevating…" : "Elevate"}
              </Button>
            </form>
          )}
        </section>
      ) : null}

      {user ? (
        <section className="space-y-3">
          <div className="space-y-1">
            <h2 className="text-lg font-semibold text-brick">Delete account</h2>
            <p className="text-sm text-ink/55">
              Removes your wishlist, store links, and sessions. Acquired library
              titles remain on this host.
            </p>
          </div>
          <div className="max-w-xl space-y-3 border border-brick/20 bg-brick/5 px-4 py-4">
            <Input
              aria-label="Type delete to confirm"
              placeholder='Type "delete" to confirm'
              value={deleteConfirm}
              onChange={(e) => setDeleteConfirm(e.target.value)}
              disabled={busy}
              autoComplete="off"
            />
            <Button
              type="button"
              variant="danger"
              disabled={busy || deleteConfirm.trim().toLowerCase() !== "delete"}
              onClick={() => void onDeleteAccount()}
            >
              <Trash2 className="h-4 w-4" />
              Delete my account
            </Button>
          </div>
        </section>
      ) : null}
    </div>
  );
}
