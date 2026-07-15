import { useCallback, useEffect, useState } from "react";
import { FileText, Film, Play } from "lucide-react";
import { fetchRecordings, logDownloadUrl, videoUrl, type RecordingInfo } from "../api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
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

export function RecordingsPage() {
  const [recordings, setRecordings] = useState<RecordingInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [playing, setPlaying] = useState<string | null>(null);

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
  if (!recordings) return null;

  if (recordings.length === 0) {
    return (
      <Card className="items-center gap-1 py-12 text-center">
        <Film className="text-muted-foreground mb-1 size-6" />
        <p className="text-sm font-medium">No recordings yet</p>
        <p className="text-muted-foreground max-w-md text-sm">
          Past sessions appear here once they leave an artifact. Start a session with the{" "}
          <code className="font-mono">enableVideo</code> capability to capture a recording you can
          revisit.
        </p>
      </Card>
    );
  }

  return (
    <div className="space-y-3">
      {recordings.map((r) => {
        const isPlaying = playing === r.id;
        return (
          <Card key={r.id} className="gap-0 overflow-hidden py-0">
            <div className="flex flex-wrap items-center gap-3 p-4">
              <div className="bg-muted flex size-9 shrink-0 items-center justify-center rounded-lg">
                <Film className="text-muted-foreground size-4" />
              </div>
              <div className="min-w-0 flex-1">
                <div className="font-mono text-sm break-all">{r.id}</div>
                <div className="text-muted-foreground text-xs">
                  {r.modified || "unknown time"}
                  {r.video && ` · ${humanBytes(r.videoBytes)}`}
                </div>
              </div>
              <div className="flex items-center gap-2">
                {r.video && <Badge variant="secondary">video</Badge>}
                {r.log && <Badge variant="secondary">logs</Badge>}
                {r.video && (
                  <Button
                    size="sm"
                    variant={isPlaying ? "secondary" : "default"}
                    onClick={() => setPlaying(isPlaying ? null : r.id)}
                  >
                    <Play className="size-4" />
                    {isPlaying ? "Hide" : "Play"}
                  </Button>
                )}
                {r.log && (
                  <Button size="sm" variant="outline" asChild>
                    <a href={logDownloadUrl(r.id)} target="_blank" rel="noreferrer">
                      <FileText className="size-4" />
                      Logs
                    </a>
                  </Button>
                )}
              </div>
            </div>
            {isPlaying && r.video && (
              <>
                <Separator />
                <div className="bg-black p-4">
                  {/* eslint-disable-next-line jsx-a11y/media-has-caption */}
                  <video
                    className="mx-auto max-h-[70vh] w-full rounded-md"
                    src={videoUrl(r.id)}
                    controls
                    autoPlay
                  />
                </div>
              </>
            )}
          </Card>
        );
      })}
    </div>
  );
}
