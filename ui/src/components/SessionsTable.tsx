import type { SessionInfo } from "../api";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

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
    return (
      <p className="text-muted-foreground py-12 text-center text-sm">No active sessions.</p>
    );
  }
  return (
    <Card className="overflow-hidden py-0">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Session</TableHead>
            <TableHead>Browser</TableHead>
            <TableHead>Owner</TableHead>
            <TableHead>Age</TableHead>
            <TableHead>Capabilities</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {sessions.map((s) => (
            <TableRow
              key={s.id}
              onClick={() => onSelect(s)}
              className="cursor-pointer"
            >
              <TableCell className="font-mono text-xs">{s.id}</TableCell>
              <TableCell>
                {s.browser} {s.version}
              </TableCell>
              <TableCell>
                {s.owner ?? <span className="text-muted-foreground">–</span>}
              </TableCell>
              <TableCell title={s.started}>{age(s.started)}</TableCell>
              <TableCell>
                <div className="flex gap-1">
                  {s.vnc && <Badge variant="secondary">vnc</Badge>}
                  {s.devtools && <Badge variant="secondary">devtools</Badge>}
                  {s.logs && <Badge variant="secondary">logs</Badge>}
                </div>
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </Card>
  );
}
