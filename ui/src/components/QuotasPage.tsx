import { useCallback, useEffect, useState } from "react";
import { fetchQuotas, putQuotas, type QuotasResponse } from "../api";

export function QuotasPage() {
  const [quotas, setQuotas] = useState<QuotasResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [newUser, setNewUser] = useState("");
  const [newLimit, setNewLimit] = useState(1);

  const refresh = useCallback(() => {
    fetchQuotas()
      .then((q) => {
        setQuotas(q);
        setError(null);
      })
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(refresh, [refresh]);

  const save = (next: { default: number; users: Record<string, number> }) =>
    putQuotas(next)
      .then(() => {
        setError(null);
        refresh();
      })
      .catch((e) => setError(String(e)));

  if (!quotas) return error ? <div className="error">{error}</div> : null;

  const rows = Object.keys(quotas.users).sort();
  return (
    <div>
      {error && <div className="error">{error}</div>}
      <div className="toolbar">
        <label>
          Default limit (0 = unlimited){" "}
          <input
            type="number"
            min={0}
            value={quotas.default}
            onChange={(e) =>
              save({ default: Number(e.target.value), users: quotas.users })
            }
          />
        </label>
      </div>
      <div className="toolbar">
        <input placeholder="user" value={newUser} onChange={(e) => setNewUser(e.target.value)} />
        <input
          type="number"
          min={0}
          value={newLimit}
          onChange={(e) => setNewLimit(Number(e.target.value))}
        />
        <button
          onClick={() => {
            if (!newUser) return;
            save({ default: quotas.default, users: { ...quotas.users, [newUser]: newLimit } });
            setNewUser("");
          }}
        >
          Set quota
        </button>
      </div>
      {rows.length === 0 ? (
        <p className="muted empty">No per-user quotas; the default applies to everyone.</p>
      ) : (
        <table className="sessions">
          <thead>
            <tr>
              <th>User</th>
              <th>Limit</th>
              <th>In use</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {rows.map((u) => (
              <tr key={u}>
                <td>{u}</td>
                <td>{quotas.users[u]}</td>
                <td>{quotas.inUse[u] ?? 0}</td>
                <td className="row-actions">
                  <button
                    onClick={() => {
                      const users = { ...quotas.users };
                      delete users[u];
                      save({ default: quotas.default, users });
                    }}
                  >
                    Remove
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
