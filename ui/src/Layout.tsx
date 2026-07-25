import { useCallback, useEffect, useRef, useState } from "react";
import { NavLink, Outlet, useLocation } from "react-router-dom";
import { Activity, Boxes, Film, Gauge, Power, RefreshCw, Users } from "lucide-react";
import {
  fetchState,
  reloadConfig,
  setDrain,
  subscribe,
  type StateResponse,
  type Stats,
} from "./api";
import type { LayoutContext } from "./layoutContext";
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
import { Separator } from "@/components/ui/separator";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import { CapacityTiles } from "./components/CapacityTiles";
import { ErrorAlert } from "./components/ErrorAlert";

const NAV = [
  { to: "sessions", label: "Sessions", icon: Activity },
  { to: "recordings", label: "Recordings", icon: Film },
  { to: "browsers", label: "Browsers", icon: Boxes },
  { to: "users", label: "Users", icon: Users },
  { to: "quotas", label: "Quotas", icon: Gauge },
] as const;

export function Layout() {
  const [state, setState] = useState<StateResponse | null>(null);
  const [stats, setStats] = useState<Stats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { pathname } = useLocation();
  // Monotonic request id: only the latest fetch is allowed to write, so
  // a slow response that lands after a newer one can't clobber it.
  const reqSeq = useRef(0);

  const refresh = useCallback(() => {
    const seq = ++reqSeq.current;
    return fetchState()
      .then((s) => {
        if (seq !== reqSeq.current) return;
        setState(s);
        setError(null);
      })
      .catch((e) => {
        if (seq !== reqSeq.current) return;
        setError(String(e));
      });
  }, []);

  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    // Belt-and-suspenders poll: SSE delivers lifecycle events instantly,
    // but a session can sit "pending" for seconds while its container
    // boots, and a dropped/reconnecting stream could miss an event. A
    // periodic snapshot keeps the session list — and the age column —
    // live regardless. Self-scheduling (not setInterval) so a slow or
    // stalled request can't stack up more in-flight fetches.
    const poll = () => {
      refresh().finally(() => {
        if (active) timer = setTimeout(poll, 3000);
      });
    };
    poll();
    const unsubscribe = subscribe({
      onStats: setStats,
      // Lifecycle events carry only deltas; the snapshot has the
      // enriched per-session flags, so refetch on change.
      onSessionCreated: refresh,
      onSessionStopped: refresh,
      onReset: refresh,
      onDrain: refresh,
      onConfigReloaded: refresh,
    });
    return () => {
      active = false;
      if (timer) clearTimeout(timer);
      unsubscribe();
    };
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
    <SidebarProvider>
      <Sidebar collapsible="icon">
        <SidebarHeader>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton size="lg" className="cursor-default hover:bg-transparent">
                <div className="bg-primary text-primary-foreground flex aspect-square size-8 items-center justify-center rounded-lg text-sm font-bold">
                  m
                </div>
                <div className="grid flex-1 text-left leading-tight">
                  <span className="font-semibold">mica</span>
                  <span className="text-muted-foreground text-xs">admin</span>
                </div>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarHeader>

        <SidebarContent>
          <SidebarGroup>
            <SidebarGroupContent>
              <SidebarMenu>
                {NAV.map(({ to, label, icon: Icon }) => (
                  <SidebarMenuItem key={to}>
                    <SidebarMenuButton
                      asChild
                      tooltip={label}
                      isActive={pathname.startsWith(`/${to}`)}
                    >
                      <NavLink to={to}>
                        <Icon />
                        <span>{label}</span>
                      </NavLink>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                ))}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        </SidebarContent>

        <SidebarFooter>
          <SidebarMenu>
            <SidebarMenuItem>
              {draining ? (
                // Resume re-enables placement — a safe recovery action,
                // no confirmation needed.
                <SidebarMenuButton tooltip="Resume" onClick={act(() => setDrain(false))}>
                  <Power />
                  <span>Resume</span>
                </SidebarMenuButton>
              ) : (
                // Draining rejects new sessions node-wide until resumed,
                // so confirm before entering it.
                <AlertDialog>
                  <AlertDialogTrigger asChild>
                    <SidebarMenuButton tooltip="Drain">
                      <Power />
                      <span>Drain</span>
                    </SidebarMenuButton>
                  </AlertDialogTrigger>
                  <AlertDialogContent>
                    <AlertDialogHeader>
                      <AlertDialogTitle>Drain this node?</AlertDialogTitle>
                      <AlertDialogDescription>
                        New sessions will be rejected and <code>/readyz</code> will report 503 until
                        you resume. Running sessions keep going.
                      </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                      <AlertDialogCancel>Cancel</AlertDialogCancel>
                      <AlertDialogAction onClick={act(() => setDrain(true))}>Drain</AlertDialogAction>
                    </AlertDialogFooter>
                  </AlertDialogContent>
                </AlertDialog>
              )}
            </SidebarMenuItem>
            <SidebarMenuItem>
              <SidebarMenuButton tooltip="Reload config" onClick={act(reloadConfig)}>
                <RefreshCw />
                <span>Reload config</span>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarFooter>
      </Sidebar>

      <SidebarInset>
        <header className="flex h-14 shrink-0 items-center gap-2 border-b px-4">
          <SidebarTrigger className="-ml-1" />
          <Separator orientation="vertical" className="mr-1 h-4" />
          <h1 className="text-base font-semibold">{title}</h1>
          {draining && (
            <Badge variant="outline" className="border-destructive/40 text-destructive">
              draining
            </Badge>
          )}
        </header>

        {/* Full-width content with a generous cap so it fills normal
            screens but doesn't stretch uncomfortably wide on ultra-wide
            monitors. */}
        <div className="mx-auto w-full max-w-[100rem] space-y-6 p-4 md:p-6">
          {error && <ErrorAlert message={error} />}
          <CapacityTiles capacity={stats ?? state?.capacity ?? null} />
          <Outlet context={context} />
        </div>
      </SidebarInset>
    </SidebarProvider>
  );
}
