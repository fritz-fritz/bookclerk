import { useMemo, useState, type Dispatch, type FormEvent, type SetStateAction } from "react";
import {
  Copy,
  Headphones,
  MoreHorizontal,
  Search,
  UserPlus,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  bootstrapAdministrator,
  createUser,
  deleteUser,
  isApiError,
  mintUserClaimTicket,
  patchUser,
  resetUserPassword,
  startImpersonate,
  type AuthSession,
  type ListedUser,
} from "@/lib/api";
import { cn } from "@/lib/utils";

const EMPTY_USER_FORM = {
  role: "member",
  display_name: "",
  login_name: "",
  password: "",
  mint_invite: true,
};

const EMPTY_BOOTSTRAP_FORM = {
  display_name: "",
  login_name: "",
  password: "",
};

const selectClassName =
  "h-9 w-full rounded-md border border-ink/15 bg-white/80 px-2 text-sm text-ink outline-none focus:border-teal";

function formatWhen(iso: string | null | undefined): string {
  if (!iso) return "—";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

function statusBadgeClass(status: string): string {
  switch (status) {
    case "active":
      return "bg-teal/15 text-ink";
    case "disabled":
      return "bg-brick/10 text-brick";
    default:
      return "bg-ink/10 text-ink/70";
  }
}

function initials(user: ListedUser): string {
  const raw = user.display_name?.trim() || user.login_name?.trim() || `U${user.id}`;
  return raw.charAt(0).toUpperCase();
}

/**
 * Administrator / operator user provisioning table with presence + integrations.
 */
export function UserManagementPanel({
  users,
  setUsers,
  busy,
  setBusy,
  error,
  setError,
  showBootstrap,
  showOperatorChrome,
  session,
  adminCount,
  currentUserId,
  onSessionChange,
  onUsersChanged,
}: {
  users: ListedUser[];
  setUsers: Dispatch<SetStateAction<ListedUser[]>>;
  busy: boolean;
  setBusy: (v: boolean) => void;
  error: string | null;
  setError: (v: string | null) => void;
  showBootstrap: boolean;
  showOperatorChrome: boolean;
  session: AuthSession | null;
  adminCount: number;
  currentUserId?: number;
  onSessionChange?: () => void | Promise<void>;
  onUsersChanged: () => void | Promise<void>;
}) {
  const [createForm, setCreateForm] = useState(EMPTY_USER_FORM);
  const [bootstrapForm, setBootstrapForm] = useState(EMPTY_BOOTSTRAP_FORM);
  const [claimTicket, setClaimTicket] = useState<{
    ticket: string;
    label: string;
  } | null>(null);
  const [query, setQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState<"all" | "active" | "disabled">(
    "all",
  );
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [impersonateBusy, setImpersonateBusy] = useState(false);
  const [menuOpenId, setMenuOpenId] = useState<number | null>(null);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return users.filter((u) => {
      if (statusFilter !== "all" && u.status !== statusFilter) return false;
      if (!q) return true;
      const hay = [
        u.display_name,
        u.login_name,
        u.role,
        String(u.id),
        ...(u.integrations ?? []).flatMap((i) => [i.source, i.label, i.account_id]),
      ]
        .filter(Boolean)
        .join(" ")
        .toLowerCase();
      return hay.includes(q);
    });
  }, [users, query, statusFilter]);

  const selected = users.find((u) => u.id === selectedId) ?? null;

  function isDeleteDisabled(u: ListedUser): boolean {
    if (busy) return true;
    if (currentUserId != null && u.id === currentUserId) return true;
    if (
      u.role === "administrator" &&
      u.status === "active" &&
      adminCount <= 1
    ) {
      return true;
    }
    return false;
  }

  async function copyClaimTicket() {
    if (!claimTicket) return;
    try {
      await navigator.clipboard.writeText(claimTicket.ticket);
    } catch {
      /* ignore */
    }
  }

  async function onBootstrap(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const res = await bootstrapAdministrator({
        display_name: bootstrapForm.display_name.trim() || undefined,
        login_name: bootstrapForm.login_name.trim() || undefined,
        password: bootstrapForm.password || undefined,
      });
      setClaimTicket({
        ticket: res.claim_ticket,
        label: "Bootstrap administrator claim ticket",
      });
      setBootstrapForm(EMPTY_BOOTSTRAP_FORM);
      await onUsersChanged();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Bootstrap failed");
    } finally {
      setBusy(false);
    }
  }

  async function onCreateUser(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const res = await createUser({
        role: createForm.role,
        display_name: createForm.display_name.trim() || undefined,
        login_name: createForm.login_name.trim() || undefined,
        password: createForm.password || undefined,
        mint_invite: createForm.mint_invite,
      });
      if (res.claim_ticket) {
        setClaimTicket({
          ticket: res.claim_ticket,
          label: `Invite for ${res.user.display_name || res.user.login_name || `user #${res.user.id}`}`,
        });
      }
      setCreateForm(EMPTY_USER_FORM);
      await onUsersChanged();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Create user failed");
    } finally {
      setBusy(false);
    }
  }

  async function onPatchUser(
    id: number,
    patch: { role?: string; status?: string; display_name?: string; login_name?: string },
  ) {
    setBusy(true);
    setError(null);
    try {
      const res = await patchUser(id, patch);
      setUsers((current) =>
        current.map((u) => (u.id === id ? { ...u, ...res.user } : u)),
      );
    } catch (err) {
      if (isApiError(err) && err.status === 409) {
        setError("Cannot demote or disable the last active administrator.");
      } else {
        setError(err instanceof Error ? err.message : "Update failed");
      }
      await onUsersChanged();
    } finally {
      setBusy(false);
    }
  }

  async function onResetUserPassword(u: ListedUser) {
    setBusy(true);
    setError(null);
    try {
      const res = await resetUserPassword(u.id);
      setClaimTicket({
        ticket: res.claim_ticket,
        label: `Password reset for ${u.display_name || u.login_name || `user #${u.id}`}`,
      });
    } catch (err) {
      setError(err instanceof Error ? err.message : "Password reset failed");
    } finally {
      setBusy(false);
    }
  }

  async function onMintClaimTicket(u: ListedUser) {
    setBusy(true);
    setError(null);
    try {
      const res = await mintUserClaimTicket(u.id);
      setClaimTicket({
        ticket: res.claim_ticket,
        label: `Invite for ${u.display_name || u.login_name || `user #${u.id}`}`,
      });
    } catch (err) {
      setError(err instanceof Error ? err.message : "Remint failed");
    } finally {
      setBusy(false);
    }
  }

  async function onDeleteUser(u: ListedUser) {
    if (
      !window.confirm(
        `Delete ${u.display_name || u.login_name || `user #${u.id}`}? Wishlist and store links are removed; acquired books stay.`,
      )
    ) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await deleteUser(u.id);
      if (selectedId === u.id) setSelectedId(null);
      await onUsersChanged();
    } catch (err) {
      if (isApiError(err) && err.status === 409) {
        setError("Cannot delete the last active administrator.");
      } else {
        setError(err instanceof Error ? err.message : "Delete failed");
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-4">
      <div className="space-y-1">
        <h2 className="text-lg font-semibold text-ink">Users</h2>
        <p className="text-sm text-ink/55">
          Provision accounts, review store integrations, and see who is signed in
          or actively listening.
        </p>
      </div>

      {error ? (
        <p className="text-sm font-medium text-brick" role="alert">
          {error}
        </p>
      ) : null}

      {claimTicket ? (
        <div className="rounded-md border border-teal/25 bg-teal/10 px-3 py-2 text-sm text-ink">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div>
              <p className="font-medium">{claimTicket.label}</p>
              <p className="font-mono text-xs text-ink/70">{claimTicket.ticket}</p>
            </div>
            <Button
              type="button"
              variant="secondary"
              className="h-8"
              onClick={() => void copyClaimTicket()}
            >
              <Copy className="h-4 w-4" />
              Copy
            </Button>
          </div>
        </div>
      ) : null}

      {showBootstrap ? (
        <form
          className="grid gap-3 bg-white/35 px-3 py-3 sm:grid-cols-[1fr_1fr_1fr_auto]"
          onSubmit={(e) => void onBootstrap(e)}
        >
          <Input
            aria-label="Administrator login name"
            value={bootstrapForm.login_name}
            onChange={(e) =>
              setBootstrapForm((c) => ({ ...c, login_name: e.target.value }))
            }
            placeholder="login name"
            autoComplete="off"
          />
          <Input
            aria-label="Administrator display name"
            value={bootstrapForm.display_name}
            onChange={(e) =>
              setBootstrapForm((c) => ({ ...c, display_name: e.target.value }))
            }
            placeholder="display name"
            autoComplete="off"
          />
          <Input
            aria-label="Administrator password"
            type="password"
            value={bootstrapForm.password}
            onChange={(e) =>
              setBootstrapForm((c) => ({ ...c, password: e.target.value }))
            }
            placeholder="optional password"
            autoComplete="new-password"
          />
          <Button type="submit" disabled={busy}>
            <UserPlus className="h-4 w-4" />
            {busy ? "Bootstrapping…" : "Bootstrap"}
          </Button>
        </form>
      ) : (
        <form
          className="grid gap-3 bg-white/35 px-3 py-3 sm:grid-cols-[9rem_1fr_1fr_1fr_auto_auto]"
          onSubmit={(e) => void onCreateUser(e)}
        >
          <select
            aria-label="New user role"
            value={createForm.role}
            onChange={(e) =>
              setCreateForm((c) => ({ ...c, role: e.target.value }))
            }
            className={selectClassName}
          >
            <option value="member">Member</option>
            <option value="administrator">Administrator</option>
          </select>
          <Input
            aria-label="New user login name"
            value={createForm.login_name}
            onChange={(e) =>
              setCreateForm((c) => ({ ...c, login_name: e.target.value }))
            }
            placeholder="login name"
            autoComplete="off"
          />
          <Input
            aria-label="New user display name"
            value={createForm.display_name}
            onChange={(e) =>
              setCreateForm((c) => ({ ...c, display_name: e.target.value }))
            }
            placeholder="display name"
            autoComplete="off"
          />
          <Input
            aria-label="New user password"
            type="password"
            value={createForm.password}
            onChange={(e) =>
              setCreateForm((c) => ({ ...c, password: e.target.value }))
            }
            placeholder="optional password"
            autoComplete="new-password"
          />
          <label className="flex items-center gap-2 text-xs text-ink/70">
            <input
              type="checkbox"
              className="h-4 w-4 accent-teal"
              checked={createForm.mint_invite}
              onChange={(e) =>
                setCreateForm((c) => ({ ...c, mint_invite: e.target.checked }))
              }
            />
            Invite
          </label>
          <Button type="submit" disabled={busy}>
            <UserPlus className="h-4 w-4" />
            {busy ? "Creating…" : "Add user"}
          </Button>
        </form>
      )}

      <div className="flex flex-wrap items-center gap-2">
        <div className="relative min-w-[14rem] flex-1">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-ink/40" />
          <Input
            className="pl-8"
            aria-label="Search users"
            placeholder="Search name, login, or integration…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <select
          aria-label="Filter by status"
          className={cn(selectClassName, "w-auto min-w-[8rem]")}
          value={statusFilter}
          onChange={(e) =>
            setStatusFilter(e.target.value as "all" | "active" | "disabled")
          }
        >
          <option value="all">All statuses</option>
          <option value="active">Active</option>
          <option value="disabled">Disabled</option>
        </select>
        <p className="text-xs text-ink/45">
          {filtered.length} of {users.length} users
        </p>
      </div>

      <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_20rem]">
        <div className="overflow-hidden rounded-md border border-ink/10 bg-white/40">
          <div className="hidden grid-cols-[minmax(12rem,1.4fr)_6rem_7rem_minmax(8rem,1fr)_7rem_2.5rem] gap-2 border-b border-ink/10 px-3 py-2 text-[11px] font-semibold uppercase tracking-wide text-ink/45 sm:grid">
            <span>User</span>
            <span>Status</span>
            <span>Role</span>
            <span>Integrations</span>
            <span>Activity</span>
            <span className="sr-only">Actions</span>
          </div>
          <ul className="divide-y divide-ink/10">
            {filtered.length === 0 ? (
              <li className="px-3 py-8 text-center text-sm text-ink/50">
                No users match this filter.
              </li>
            ) : (
              filtered.map((u) => {
                const selectedRow = selectedId === u.id;
                return (
                  <li key={u.id}>
                    <button
                      type="button"
                      onClick={() => setSelectedId(u.id)}
                      className={cn(
                        "grid w-full gap-2 px-3 py-3 text-left text-sm transition-colors sm:grid-cols-[minmax(12rem,1.4fr)_6rem_7rem_minmax(8rem,1fr)_7rem_2.5rem]",
                        selectedRow
                          ? "border-l-4 border-l-teal bg-teal/10"
                          : "border-l-4 border-l-transparent hover:bg-ink/[0.03]",
                      )}
                    >
                      <div className="flex min-w-0 items-center gap-3">
                        <div className="relative flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-ink/10 text-sm font-semibold text-ink">
                          {initials(u)}
                          <span
                            className={cn(
                              "absolute bottom-0 right-0 h-2.5 w-2.5 rounded-full border-2 border-paper",
                              u.online ? "bg-teal" : "bg-ink/25",
                            )}
                            title={u.online ? "Signed in" : "Offline"}
                          />
                        </div>
                        <div className="min-w-0">
                          <p className="truncate font-medium text-ink">
                            {u.display_name?.trim() || `User #${u.id}`}
                          </p>
                          <p className="truncate text-xs text-ink/50">
                            {u.login_name || "no login"} · #{u.id}
                          </p>
                        </div>
                      </div>
                      <div className="flex items-center">
                        <Badge
                          className={cn(
                            "normal-case tracking-normal",
                            statusBadgeClass(u.status),
                          )}
                        >
                          {u.status}
                        </Badge>
                      </div>
                      <div className="flex items-center text-ink/70">
                        {u.role === "administrator" ? "Admin" : "Member"}
                      </div>
                      <div className="flex flex-wrap items-center gap-1">
                        {(u.integrations ?? []).length === 0 ? (
                          <span className="text-xs text-ink/40">None</span>
                        ) : (
                          (u.integrations ?? []).slice(0, 3).map((i) => (
                            <Badge
                              key={`${i.source}:${i.account_id}`}
                              className="bg-ink/8 text-ink/75 normal-case tracking-normal"
                            >
                              {i.source}
                            </Badge>
                          ))
                        )}
                        {(u.integrations ?? []).length > 3 ? (
                          <span className="text-xs text-ink/45">
                            +{(u.integrations ?? []).length - 3}
                          </span>
                        ) : null}
                      </div>
                      <div className="min-w-0 space-y-1">
                        {u.listening ? (
                          <div className="flex items-start gap-1.5 text-xs text-teal">
                            <Headphones className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                            <span className="line-clamp-2">
                              {u.listening.title?.trim() || "Listening"}
                              <span className="text-ink/45">
                                {" "}
                                · {u.listening.provider}
                              </span>
                            </span>
                          </div>
                        ) : (
                          <p className="text-xs text-ink/45">
                            {u.online ? "Online" : "Offline"}
                          </p>
                        )}
                        <p className="text-[11px] text-ink/40">
                          {formatWhen(u.last_active_at)}
                        </p>
                      </div>
                      <div
                        className="relative flex items-start justify-end"
                        onClick={(e) => e.stopPropagation()}
                      >
                        <Button
                          type="button"
                          variant="ghost"
                          className="h-8 w-8 p-0"
                          aria-label={`Actions for user ${u.id}`}
                          onClick={() =>
                            setMenuOpenId((cur) => (cur === u.id ? null : u.id))
                          }
                        >
                          <MoreHorizontal className="h-4 w-4" />
                        </Button>
                        {menuOpenId === u.id ? (
                          <div className="absolute right-0 top-9 z-20 min-w-[11rem] rounded-md border border-ink/10 bg-paper py-1 shadow-lg">
                            <button
                              type="button"
                              className="block w-full px-3 py-1.5 text-left text-sm hover:bg-ink/5"
                              onClick={() => {
                                setMenuOpenId(null);
                                setSelectedId(u.id);
                              }}
                            >
                              View details
                            </button>
                            <button
                              type="button"
                              className="block w-full px-3 py-1.5 text-left text-sm hover:bg-ink/5"
                              disabled={busy}
                              onClick={() => {
                                setMenuOpenId(null);
                                void onResetUserPassword(u);
                              }}
                            >
                              Reset password
                            </button>
                            <button
                              type="button"
                              className="block w-full px-3 py-1.5 text-left text-sm hover:bg-ink/5"
                              disabled={busy || u.status === "disabled"}
                              onClick={() => {
                                setMenuOpenId(null);
                                void onMintClaimTicket(u);
                              }}
                            >
                              Remint invite
                            </button>
                            {showOperatorChrome ? (
                              <button
                                type="button"
                                className="block w-full px-3 py-1.5 text-left text-sm hover:bg-ink/5"
                                disabled={
                                  impersonateBusy ||
                                  session?.impersonating?.user_id === u.id
                                }
                                onClick={() => {
                                  setMenuOpenId(null);
                                  void (async () => {
                                    setImpersonateBusy(true);
                                    try {
                                      await startImpersonate(u.id);
                                      await onSessionChange?.();
                                    } catch (err) {
                                      setError(
                                        err instanceof Error
                                          ? err.message
                                          : "Impersonate failed",
                                      );
                                    } finally {
                                      setImpersonateBusy(false);
                                    }
                                  })();
                                }}
                              >
                                Impersonate
                              </button>
                            ) : null}
                            <button
                              type="button"
                              className="block w-full px-3 py-1.5 text-left text-sm text-brick hover:bg-brick/5 disabled:opacity-40"
                              disabled={isDeleteDisabled(u)}
                              onClick={() => {
                                setMenuOpenId(null);
                                void onDeleteUser(u);
                              }}
                            >
                              Delete
                            </button>
                          </div>
                        ) : null}
                      </div>
                    </button>
                  </li>
                );
              })
            )}
          </ul>
        </div>

        <aside className="rounded-md border border-ink/10 bg-white/40 p-4">
          {selected ? (
            <div className="space-y-4">
              <div className="flex items-start justify-between gap-2">
                <div>
                  <p className="text-xs font-semibold uppercase tracking-wide text-ink/45">
                    User information
                  </p>
                  <div className="mt-2 flex items-center gap-3">
                    <div className="relative flex h-12 w-12 items-center justify-center rounded-full bg-ink/10 text-base font-semibold">
                      {initials(selected)}
                      <span
                        className={cn(
                          "absolute bottom-0.5 right-0.5 h-3 w-3 rounded-full border-2 border-paper",
                          selected.online ? "bg-teal" : "bg-ink/25",
                        )}
                      />
                    </div>
                    <div>
                      <p className="font-semibold text-ink">
                        {selected.display_name?.trim() || `User #${selected.id}`}
                      </p>
                      <p className="text-sm text-ink/55">
                        {selected.login_name || "no login"}
                      </p>
                    </div>
                  </div>
                </div>
                <Badge
                  className={cn(
                    "normal-case tracking-normal",
                    statusBadgeClass(selected.status),
                  )}
                >
                  {selected.status}
                </Badge>
              </div>

              {selected.listening ? (
                <div className="rounded-md bg-teal/10 px-3 py-2 text-sm text-ink">
                  <div className="flex items-center gap-2 font-medium">
                    <Headphones className="h-4 w-4 text-teal" />
                    Listening now
                  </div>
                  <p className="mt-1 text-ink/70">
                    {selected.listening.title?.trim() || "Untitled"}
                    <span className="text-ink/45">
                      {" "}
                      · {selected.listening.provider}
                    </span>
                  </p>
                </div>
              ) : null}

              <div className="space-y-2">
                <label className="block text-xs font-semibold uppercase tracking-wide text-ink/45">
                  Role
                </label>
                <select
                  aria-label={`Role for user ${selected.id}`}
                  value={selected.role}
                  disabled={busy}
                  onChange={(e) =>
                    void onPatchUser(selected.id, { role: e.target.value })
                  }
                  className={selectClassName}
                >
                  <option value="member">Member</option>
                  <option value="administrator">Administrator</option>
                </select>
              </div>
              <div className="space-y-2">
                <label className="block text-xs font-semibold uppercase tracking-wide text-ink/45">
                  Status
                </label>
                <select
                  aria-label={`Status for user ${selected.id}`}
                  value={selected.status}
                  disabled={busy}
                  onChange={(e) =>
                    void onPatchUser(selected.id, { status: e.target.value })
                  }
                  className={selectClassName}
                >
                  <option value="active">Active</option>
                  <option value="disabled">Disabled</option>
                </select>
              </div>
              <div className="space-y-2">
                <label className="block text-xs font-semibold uppercase tracking-wide text-ink/45">
                  Display name
                </label>
                <Input
                  aria-label={`Display name for user ${selected.id}`}
                  value={selected.display_name ?? ""}
                  disabled={busy}
                  onChange={(e) => {
                    const value = e.target.value;
                    setUsers((current) =>
                      current.map((user) =>
                        user.id === selected.id
                          ? { ...user, display_name: value }
                          : user,
                      ),
                    );
                  }}
                  onBlur={(e) =>
                    void onPatchUser(selected.id, {
                      display_name: e.target.value,
                    })
                  }
                />
              </div>
              <div className="space-y-2">
                <label className="block text-xs font-semibold uppercase tracking-wide text-ink/45">
                  Login name
                </label>
                <Input
                  aria-label={`Login name for user ${selected.id}`}
                  value={selected.login_name ?? ""}
                  disabled={busy}
                  onChange={(e) => {
                    const value = e.target.value;
                    setUsers((current) =>
                      current.map((user) =>
                        user.id === selected.id
                          ? { ...user, login_name: value }
                          : user,
                      ),
                    );
                  }}
                  onBlur={(e) =>
                    void onPatchUser(selected.id, { login_name: e.target.value })
                  }
                />
              </div>

              <div className="space-y-2">
                <p className="text-xs font-semibold uppercase tracking-wide text-ink/45">
                  Integrations ({(selected.integrations ?? []).length})
                </p>
                {(selected.integrations ?? []).length === 0 ? (
                  <p className="text-sm text-ink/50">No store connections.</p>
                ) : (
                  <ul className="flex flex-wrap gap-1.5">
                    {(selected.integrations ?? []).map((i) => (
                      <li key={`${i.source}:${i.account_id}`}>
                        <Badge className="bg-ink/8 text-ink normal-case tracking-normal">
                          {i.source}
                          {i.label ? ` · ${i.label}` : ""}
                        </Badge>
                      </li>
                    ))}
                  </ul>
                )}
              </div>

              <p className="text-xs text-ink/45">
                Password: {selected.has_password ? "set" : "not set"}
                {" · "}
                Last active {formatWhen(selected.last_active_at)}
              </p>
            </div>
          ) : (
            <p className="text-sm text-ink/50">
              Select a user to edit role, status, and review integrations.
            </p>
          )}
        </aside>
      </div>
    </div>
  );
}
