import { cn } from "@/lib/utils";
import type { AppView, AuthRole } from "@/lib/api";
import { pathForView } from "@/lib/routes";

/**
 * Props for {@link AppNav}.
 */
export interface AppNavProps {
  /** Currently active app view. */
  view: AppView;
  /** In-app navigation callback (history is updated by the parent). */
  onNavigate: (view: AppView) => void;
  /** Effective session role — Settings is operator/administrator only. */
  role?: AuthRole;
}

const BASE_LINKS: { id: AppView; label: string }[] = [
  { id: "discover", label: "Discover" },
  { id: "wishlist", label: "Wishlist" },
  { id: "library", label: "Library" },
  { id: "accounts", label: "Accounts" },
];

/**
 * Whether Settings appears in primary nav for this role.
 *
 * Members (including when an operator is impersonating a member) do not get
 * Settings — preferences live under the gear control instead.
 */
export function showSettingsNav(role?: AuthRole): boolean {
  return role === "operator" || role === "administrator";
}

/**
 * Primary nav links for an effective role (Settings gated).
 */
export function navLinksForRole(
  role?: AuthRole,
): { id: AppView; label: string }[] {
  if (showSettingsNav(role)) {
    return [...BASE_LINKS, { id: "settings", label: "Settings" }];
  }
  return BASE_LINKS;
}

/**
 * Primary in-app navigation links for authenticated views.
 *
 * @param props - Current view and navigate callback.
 */
export function AppNav({ view, onNavigate, role }: AppNavProps) {
  return (
    <nav className="flex flex-wrap items-center gap-1 text-sm sm:gap-2">
      {navLinksForRole(role).map((link) => (
        <a
          key={link.id}
          href={pathForView(link.id)}
          onClick={(e) => {
            // Keep in-app navigation instant; allow modified clicks to open a tab.
            if (
              e.defaultPrevented ||
              e.button !== 0 ||
              e.metaKey ||
              e.ctrlKey ||
              e.shiftKey ||
              e.altKey
            ) {
              return;
            }
            e.preventDefault();
            onNavigate(link.id);
          }}
          className={cn(
            "rounded-md px-2 py-1 transition-colors",
            view === link.id
              ? "font-semibold text-ink"
              : "text-ink/55 hover:bg-ink/5 hover:text-ink",
          )}
          aria-current={view === link.id ? "page" : undefined}
        >
          {link.label}
        </a>
      ))}
    </nav>
  );
}
