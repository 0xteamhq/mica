import type { SessionInfo } from "../api";

function age(started: string): string {
  const ms = Date.now() - Date.parse(started);
  if (Number.isNaN(ms)) return "?";
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}

export function SessionsTable({
  sessions,
  onSelect,
}: {
  sessions: SessionInfo[];
  onSelect: (s: SessionInfo) => void;
}) {
  if (sessions.length === 0) {
    return <p className="muted empty">No active sessions.</p>;
  }
  return (
    <table className="sessions">
      <thead>
        <tr>
          <th>Session</th>
          <th>Browser</th>
          <th>Owner</th>
          <th>Age</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {sessions.map((s) => (
          <tr key={s.id} onClick={() => onSelect(s)}>
            <td className="mono">{s.id}</td>
            <td>
              {s.browser} {s.version}
            </td>
            <td>{s.owner ?? <span className="muted">–</span>}</td>
            <td title={s.started}>{age(s.started)}</td>
            <td className="caps">
              {s.vnc && <span className="badge">vnc</span>}
              {s.devtools && <span className="badge">devtools</span>}
              {s.logs && <span className="badge">logs</span>}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
