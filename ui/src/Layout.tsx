import { useCallback, useEffect, useState } from "react";
import { NavLink, Outlet, useLocation } from "react-router-dom";
import { Activity, Boxes, Gauge, Power, RefreshCw, Users } from "lucide-react";
import {
  fetchState,
  reloadConfig,
  setDrain,
  subscribe,
  type StateResponse,
  type Stats,
} from "./api";
import type { LayoutContext } from "./layoutContext";
import { cn } from "@/lib/utils";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { CapacityTiles } from "./components/CapacityTiles";
import { ErrorAlert } from "./components/ErrorAlert";

const NAV = [
  { to: "sessions", label: "Sessions", icon: Activity },
  { to: "browsers", label: "Browsers", icon: Boxes },
  { to: "users", label: "Users", icon: Users },
  { to: "quotas", label: "Quotas", icon: Gauge },
] as const;

export function Layout() {
  const [state, setState] = useState<StateResponse | null>(null);
  const [stats, setStats] = useState<Stats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { pathname } = useLocation();

  const refresh = useCallback(() => {
    fetchState()
      .then((s) => {
        setState(s);
        setError(null);
      })
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    refresh();
    return subscribe({
      onStats: setStats,
      // Lifecycle events carry only deltas; the snapshot has the
      // enriched per-session flags, so refetch on change.
      onSessionCreated: refresh,
      onSessionStopped: refresh,
      onReset: refresh,
      onDrain: refresh,
      onConfigReloaded: refresh,
    });
  }, [refresh]);

  const draining = stats?.draining ?? state?.draining ?? false;

  const act = (fn: () => Promise<unknown>) => () =>
    fn()
      .then(() => {
        setError(null);
        refresh();
      })
      .catch((e) => setError(String(e)));

  const title = NAV.find((n) => pathname.startsWith(`/${n.to}`))?.label ?? "";
  const context: LayoutContext = { state, refresh };

  return (
    <div className="flex min-h-screen">
      <aside className="bg-sidebar text-sidebar-foreground flex w-56 shrink-0 flex-col border-r">
        <div className="flex h-14 items-center gap-2 px-4 text-lg font-semibold">
          mica <span className="text-muted-foreground font-normal">admin</span>
        </div>
        <Separator />
        <nav className="flex-1 space-y-1 p-3">
          {NAV.map(({ to, label, icon: Icon }) => (
            <NavLink
              key={to}
              to={to}
              className={({ isActive }) =>
                cn(
                  "flex w-full items-center gap-2.5 rounded-md px-3 py-2 text-sm font-medium transition-colors",
                  isActive
                    ? "bg-sidebar-accent text-sidebar-accent-foreground"
                    : "text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground",
                )
              }
            >
              <Icon className="size-4" />
              {label}
            </NavLink>
          ))}
        </nav>
        <Separator />
        <div className="space-y-2 p-3">
          {draining ? (
            // Resume re-enables placement — a safe recovery action, no
            // confirmation needed.
            <Button
              variant="default"
              className="w-full justify-start"
              onClick={act(() => setDrain(false))}
            >
              <Power className="size-4" />
              Resume
            </Button>
          ) : (
            // Draining rejects new sessions node-wide until resumed, so
            // confirm before entering it.
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button variant="outline" className="w-full justify-start">
                  <Power className="size-4" />
                  Drain
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>Drain this node?</AlertDialogTitle>
                  <AlertDialogDescription>
                    New sessions will be rejected and <code>/readyz</code> will
                    report 503 until you resume. Running sessions keep going.
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Cancel</AlertDialogCancel>
                  <AlertDialogAction onClick={act(() => setDrain(true))}>
                    Drain
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          )}
          <Button
            variant="ghost"
            className="w-full justify-start"
            onClick={act(reloadConfig)}
          >
            <RefreshCw className="size-4" />
            Reload config
          </Button>
        </div>
      </aside>

      <main className="min-w-0 flex-1">
        <header className="flex h-14 items-center gap-3 border-b px-6">
          <h1 className="text-base font-semibold">{title}</h1>
          {draining && (
            <Badge variant="outline" className="border-destructive/40 text-destructive">
              draining
            </Badge>
          )}
        </header>

        <div className="mx-auto max-w-6xl space-y-6 p-6">
          {error && <ErrorAlert message={error} />}
          <CapacityTiles capacity={stats ?? state?.capacity ?? null} />
          <Outlet context={context} />
        </div>
      </main>
    </div>
  );
}
