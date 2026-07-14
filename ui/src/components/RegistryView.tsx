import type { BrowserInfo } from "../api";

export function RegistryView({ browsers }: { browsers: Record<string, BrowserInfo> }) {
  const names = Object.keys(browsers).sort();
  if (names.length === 0) {
    return <p className="muted empty">No browsers configured.</p>;
  }
  return (
    <table className="sessions">
      <thead>
        <tr>
          <th>Browser</th>
          <th>Default</th>
          <th>Versions</th>
        </tr>
      </thead>
      <tbody>
        {names.map((name) => (
          <tr key={name}>
            <td>{name}</td>
            <td>
              <span className="badge">{browsers[name].default}</span>
            </td>
            <td>{browsers[name].versions.join(", ")}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
