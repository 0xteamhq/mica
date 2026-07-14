import { useCallback, useEffect, useState } from "react";
import {
  fetchState,
  reloadConfig,
  setDrain,
  subscribe,
  type SessionInfo,
  type StateResponse,
  type Stats,
} from "./api";
import { CapacityTiles } from "./components/CapacityTiles";
import { SessionsTable } from "./components/SessionsTable";
import { SessionDrawer } from "./components/SessionDrawer";
import { RegistryEditor } from "./components/RegistryEditor";
import { UsersPage } from "./components/UsersPage";
import { QuotasPage } from "./components/QuotasPage";

type Tab = "sessions" | "registry" | "users" | "quotas";

export default function App() {
  const [state, setState] = useState<StateResponse | null>(null);
  const [stats, setStats] = useState<Stats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("sessions");
  const [selected, setSelected] = useState<SessionInfo | null>(null);

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

  return (
    <div className="app">
      <header>
        <h1>
          mica <span className="muted">admin</span>
        </h1>
        {draining && <span className="badge badge-warn">draining</span>}
        <nav>
          <button className={tab === "sessions" ? "active" : ""} onClick={() => setTab("sessions")}>
            Sessions
          </button>
          <button className={tab === "registry" ? "active" : ""} onClick={() => setTab("registry")}>
            Browsers
          </button>
          <button className={tab === "users" ? "active" : ""} onClick={() => setTab("users")}>
            Users
          </button>
          <button className={tab === "quotas" ? "active" : ""} onClick={() => setTab("quotas")}>
            Quotas
          </button>
          <button onClick={act(() => setDrain(!draining))}>
            {draining ? "Resume" : "Drain"}
          </button>
          <button onClick={act(reloadConfig)}>Reload config</button>
        </nav>
      </header>

      {error && <div className="error">{error}</div>}

      <CapacityTiles capacity={stats ?? state?.capacity ?? null} />

      {tab === "sessions" && (
        <SessionsTable sessions={state?.sessions ?? []} onSelect={setSelected} />
      )}
      {tab === "registry" && (
        <RegistryEditor browsers={state?.browsers ?? {}} onSaved={refresh} />
      )}
      {tab === "users" && <UsersPage />}
      {tab === "quotas" && <QuotasPage />}

      {selected && <SessionDrawer session={selected} onClose={() => setSelected(null)} />}
    </div>
  );
}
