import { useCallback, useEffect, useState } from "react";
import { fetchQuotas, putQuotas, type QuotasResponse } from "../api";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
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

  if (!quotas) return error ? <ErrorAlert message={error} /> : null;

  const rows = Object.keys(quotas.users).sort();
  return (
    <div className="space-y-4">
      {error && <ErrorAlert message={error} />}
      <Label className="gap-3">
        Default limit (0 = unlimited)
        <Input
          type="number"
          min={0}
          className="w-24"
          value={quotas.default}
          onChange={(e) => save({ default: Number(e.target.value), users: quotas.users })}
        />
      </Label>
      <div className="flex flex-wrap items-center gap-2">
        <Input
          placeholder="user"
          className="w-40"
          value={newUser}
          onChange={(e) => setNewUser(e.target.value)}
        />
        <Input
          type="number"
          min={0}
          className="w-24"
          value={newLimit}
          onChange={(e) => setNewLimit(Number(e.target.value))}
        />
        <Button
          size="sm"
          onClick={() => {
            if (!newUser) return;
            save({ default: quotas.default, users: { ...quotas.users, [newUser]: newLimit } });
            setNewUser("");
          }}
        >
          Set quota
        </Button>
      </div>
      {rows.length === 0 ? (
        <p className="text-muted-foreground py-12 text-center text-sm">
          No per-user quotas; the default applies to everyone.
        </p>
      ) : (
        <Card className="overflow-hidden py-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>User</TableHead>
                <TableHead>Limit</TableHead>
                <TableHead>In use</TableHead>
                <TableHead className="text-right">Actions</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((u) => (
                <TableRow key={u}>
                  <TableCell className="font-medium">{u}</TableCell>
                  <TableCell>{quotas.users[u]}</TableCell>
                  <TableCell>{quotas.inUse[u] ?? 0}</TableCell>
                  <TableCell className="text-right">
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => {
                        const users = { ...quotas.users };
                        delete users[u];
                        save({ default: quotas.default, users });
                      }}
                    >
                      Remove
                    </Button>
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
