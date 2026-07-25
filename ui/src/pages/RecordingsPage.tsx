import { useCallback, useEffect, useState } from "react";
import { Download, FileText, Film } from "lucide-react";
import { fetchRecordings, logDownloadUrl, videoUrl, type RecordingInfo } from "../api";
import { DataTable, type Column, type Filter } from "../components/DataTable";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Separator } from "@/components/ui/separator";
import { ErrorAlert } from "../components/ErrorAlert";

function humanBytes(n: number): string {
  if (n <= 0) return "—";
  const units = ["B", "KB", "MB", "GB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 && i > 0 ? 1 : 0)} ${units[i]}`;
}

/** RFC3339 -> localized short datetime (falls back to the raw string). */
function humanTime(iso: string): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    timeZoneName: "short",
  });
}

function cap(s: string): string {
  return s ? s.charAt(0).toUpperCase() + s.slice(1) : s;
}

export function RecordingsPage() {
  const [recordings, setRecordings] = useState<RecordingInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<RecordingInfo | null>(null);

  const refresh = useCallback(() => {
    fetchRecordings()
      .then((r) => {
        setRecordings(r);
        setError(null);
      })
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(refresh, [refresh]);

  if (error) return <ErrorAlert message={error} />;
  if (!recordings) {
    return (
      <div className="text-muted-foreground flex items-center justify-center gap-2 py-12 text-sm">
        <span
          className="border-muted-foreground/30 border-t-foreground size-4 animate-spin rounded-full border-2"
          aria-hidden
        />
        Loading recordings…
      </div>
    );
  }

  const columns: Column<RecordingInfo>[] = [
    {
      key: "id",
      header: "Session",
      cell: (r) => (
        <div className="flex items-center gap-2.5">
          <div className="bg-muted flex size-8 shrink-0 items-center justify-center rounded-md">
            <Film className="text-muted-foreground size-4" />
          </div>
          <span className="font-mono text-xs break-all">{r.id}</span>
        </div>
      ),
    },
    {
      key: "browser",
      header: "Browser",
      cell: (r) =>
        r.browser ? (
          <div className="flex flex-col leading-tight">
            <span className="capitalize">{r.browser}</span>
            {r.version && <span className="text-muted-foreground text-xs">{r.version}</span>}
          </div>
        ) : (
          <span className="text-muted-foreground">—</span>
        ),
    },
    {
      key: "platform",
      header: "Platform",
      className: "text-muted-foreground capitalize",
      cell: (r) => r.platform || "—",
    },
    {
      key: "owner",
      header: "Owner",
      className: "text-muted-foreground",
      cell: (r) => r.owner || "—",
    },
    {
      key: "artifacts",
      header: "Artifacts",
      cell: (r) => (
        <div className="flex gap-1">
          {r.video && <Badge variant="secondary">video</Badge>}
          {r.log && <Badge variant="secondary">logs</Badge>}
        </div>
      ),
    },
    {
      key: "size",
      header: "Size",
      className: "text-muted-foreground tabular-nums",
      cell: (r) => (r.video ? humanBytes(r.videoBytes) : "—"),
    },
    {
      key: "recorded",
      header: "Recorded",
      className: "text-muted-foreground",
      cell: (r) => humanTime(r.modified),
    },
    {
      key: "actions",
      header: "Actions",
      className: "text-right",
      cell: (r) => (
        <Button
          size="sm"
          variant="outline"
          onClick={(e) => {
            e.stopPropagation();
            setSelected(r);
          }}
        >
          Open
        </Button>
      ),
    },
  ];

  const filters: Filter<RecordingInfo>[] = [
    { key: "browser", label: "Browser", accessor: (r) => r.browser },
    { key: "version", label: "Version", accessor: (r) => r.version },
    { key: "platform", label: "Platform", accessor: (r) => r.platform },
  ];

  return (
    <>
      <DataTable
        rows={recordings}
        columns={columns}
        rowKey={(r) => r.id}
        filters={filters}
        searchAccessor={(r) => `${r.id} ${r.owner ?? ""}`}
        searchPlaceholder="Search session or owner"
        onRowClick={setSelected}
        empty={
          <Card className="items-center gap-1 border-0 bg-transparent py-0 shadow-none">
            <Film className="text-muted-foreground mb-1 size-6" />
            <p className="text-sm font-medium">No recordings yet</p>
            <p className="text-muted-foreground max-w-md text-sm">
              Past sessions appear here once they leave an artifact. Start a session with the{" "}
              <code className="font-mono">enableVideo</code> capability to capture a recording you
              can revisit.
            </p>
          </Card>
        }
        noMatch="No recordings match these filters."
      />

      <RecordingDialog
        recording={selected}
        onOpenChange={(open) => !open && setSelected(null)}
      />
    </>
  );
}

function RecordingDialog({
  recording,
  onOpenChange,
}: {
  recording: RecordingInfo | null;
  onOpenChange: (open: boolean) => void;
}) {
  return (
    <Dialog open={recording != null} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-3xl">
        {recording && (
          <>
            <DialogHeader className="pr-8">
              <DialogTitle className="font-mono text-sm break-all">{recording.id}</DialogTitle>
            </DialogHeader>

            {recording.video ? (
              <div className="overflow-hidden rounded-md bg-black">
                {/* eslint-disable-next-line jsx-a11y/media-has-caption */}
                <video
                  key={recording.id}
                  className="max-h-[60vh] w-full"
                  src={videoUrl(recording.id)}
                  controls
                  autoPlay
                />
              </div>
            ) : (
              <div className="text-muted-foreground rounded-md border border-dashed py-10 text-center text-sm">
                No video for this session — logs only.
              </div>
            )}

            <Separator />

            {/* Session info for reference */}
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm sm:grid-cols-[auto_1fr_auto_1fr]">
              <InfoRow label="Browser" value={cap(recording.browser) || "—"} />
              <InfoRow label="Version" value={recording.version || "—"} />
              <InfoRow label="Platform" value={cap(recording.platform) || "—"} />
              <InfoRow label="Owner" value={recording.owner || "—"} />
              <InfoRow label="Started" value={humanTime(recording.started)} />
              <InfoRow label="Recorded" value={humanTime(recording.modified)} />
              {recording.video && (
                <InfoRow label="Video size" value={humanBytes(recording.videoBytes)} />
              )}
              <InfoRow
                label="Artifacts"
                value={
                  [recording.video && "video", recording.log && "logs"]
                    .filter(Boolean)
                    .join(", ") || "none"
                }
              />
            </dl>

            <div className="flex flex-wrap gap-2">
              {recording.video && (
                <Button size="sm" variant="outline" asChild>
                  <a href={videoUrl(recording.id)} download>
                    <Download className="size-4" />
                    Download video
                  </a>
                </Button>
              )}
              {recording.log && (
                <Button size="sm" variant="outline" asChild>
                  <a href={logDownloadUrl(recording.id)} target="_blank" rel="noreferrer">
                    <FileText className="size-4" />
                    View logs
                  </a>
                </Button>
              )}
            </div>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="break-all">{value}</dd>
    </>
  );
}
