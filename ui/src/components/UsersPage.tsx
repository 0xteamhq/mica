import { useCallback, useEffect, useState } from "react";
import { deleteUser, fetchUsers, putUser, type UserEntry } from "../api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ErrorAlert } from "./ErrorAlert";

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
    <div className="space-y-4">
      {error && <ErrorAlert message={error} />}
      <div className="flex flex-wrap items-center gap-2">
        <Input
          placeholder="name"
          className="w-40"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        <Input
          placeholder="password"
          type="password"
          className="w-44"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        <Label className="px-1">
          <Checkbox
            checked={admin}
            onCheckedChange={(v) => setAdmin(v === true)}
          />
          admin
        </Label>
        <Button size="sm" onClick={add}>
          Add user
        </Button>
      </div>
      {users && users.length === 0 && (
        <p className="text-muted-foreground py-12 text-center text-sm">
          No users (auth is open or the file is empty).
        </p>
      )}
      {users && users.length > 0 && (
        <Card className="overflow-hidden py-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>User</TableHead>
                <TableHead>Role</TableHead>
                <TableHead className="text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {users.map((u) => (
                <TableRow key={u.name}>
                  <TableCell className="font-medium">{u.name}</TableCell>
                  <TableCell>
                    {u.admin ? (
                      <Badge variant="secondary">admin</Badge>
                    ) : (
                      <span className="text-muted-foreground">user</span>
                    )}
                  </TableCell>
                  <TableCell className="text-right">
                    <div className="flex justify-end gap-2">
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => act(() => putUser(u.name, { admin: !u.admin }))}
                      >
                        {u.admin ? "Demote" : "Promote"}
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => act(() => deleteUser(u.name))}
                      >
                        Delete
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </Card>
      )}
    </div>
  );
}
