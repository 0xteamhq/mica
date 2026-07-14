import { useCallback, useEffect, useState } from "react";
import { deleteUser, fetchUsers, putUser, type UserEntry } from "../api";

export function UsersPage() {
  const [users, setUsers] = useState<UserEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [admin, setAdmin] = useState(false);

  const refresh = useCallback(() => {
    fetchUsers()
      .then((u) => {
        setUsers(u);
        setError(null);
      })
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(refresh, [refresh]);

  const act = (fn: () => Promise<unknown>) =>
    fn()
      .then(() => {
        setError(null);
        refresh();
      })
      .catch((e) => setError(String(e)));

  const add = () => {
    if (!name || !password) {
      setError("users: name and password are required");
      return;
    }
    act(() => putUser(name, { password, admin })).then(() => {
      setName("");
      setPassword("");
      setAdmin(false);
    });
  };

  return (
    <div>
      {error && <div className="error">{error}</div>}
      <div className="toolbar">
        <input placeholder="name" value={name} onChange={(e) => setName(e.target.value)} />
        <input
          placeholder="password"
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        <label>
          <input type="checkbox" checked={admin} onChange={(e) => setAdmin(e.target.checked)} />{" "}
          admin
        </label>
        <button onClick={add}>Add user</button>
      </div>
      {users && users.length === 0 && (
        <p className="muted empty">No users (auth is open or the file is empty).</p>
      )}
      {users && users.length > 0 && (
        <table className="sessions">
          <thead>
            <tr>
              <th>User</th>
              <th>Role</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {users.map((u) => (
              <tr key={u.name}>
                <td>{u.name}</td>
                <td>{u.admin ? <span className="badge">admin</span> : <span className="muted">user</span>}</td>
                <td className="row-actions">
                  <button onClick={() => act(() => putUser(u.name, { admin: !u.admin }))}>
                    {u.admin ? "Demote" : "Promote"}
                  </button>
                  <button onClick={() => act(() => deleteUser(u.name))}>Delete</button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
