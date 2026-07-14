import { useEffect, useRef, useState } from "react";
import RFB from "@novnc/novnc";
import { fetchLog, killSession, vncUrl, type SessionInfo } from "../api";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { ErrorAlert } from "./ErrorAlert";

type Pane = "vnc" | "logs";

export function SessionDrawer({
  session,
  onClose,
}: {
  session: SessionInfo;
  onClose: () => void;
}) {
  const [pane, setPane] = useState<Pane>(session.vnc ? "vnc" : "logs");
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);

  const kill = () => {
    if (!confirming) {
      setConfirming(true);
      return;
    }
    killSession(session.id)
      .then(onClose)
      .catch((e) => {
        setConfirming(false);
        setError(String(e));
      });
  };

  return (
    <Sheet open onOpenChange={(o) => !o && onClose()}>
      <SheetContent side="right" className="gap-0 p-0 sm:max-w-3xl">
        <SheetHeader className="pr-12">
          <SheetTitle className="font-mono text-sm break-all">{session.id}</SheetTitle>
          <SheetDescription>
            {session.browser} {session.version} · started {session.started}
            {session.owner && <> · {session.owner}</>}
          </SheetDescription>
        </SheetHeader>
        <Separator />
        <div className="flex items-center gap-2 p-4">
          <Button
            variant={pane === "vnc" ? "default" : "outline"}
            size="sm"
            disabled={!session.vnc}
            onClick={() => setPane("vnc")}
          >
            VNC
          </Button>
          <Button
            variant={pane === "logs" ? "default" : "outline"}
            size="sm"
            onClick={() => setPane("logs")}
          >
            Logs
          </Button>
          <Button
            variant="destructive"
            size="sm"
            className="ml-auto"
            onClick={kill}
            onBlur={() => setConfirming(false)}
          >
            {confirming ? "Confirm kill" : "Kill session"}
          </Button>
        </div>
        <div className="flex min-h-0 flex-1 flex-col px-4 pb-4">
          {error && <ErrorAlert message={error} />}
          {pane === "vnc" && session.vnc && <VncPane sessionId={session.id} />}
          {pane === "logs" && <LogPane sessionId={session.id} available={session.logs} />}
        </div>
      </SheetContent>
    </Sheet>
  );
}

function VncPane({ sessionId }: { sessionId: string }) {
  const target = useRef<HTMLDivElement>(null);
  const [status, setStatus] = useState("connecting…");

  useEffect(() => {
    if (!target.current) return;
    const rfb = new RFB(target.current, vncUrl(sessionId));
    rfb.scaleViewport = true;
    rfb.viewOnly = false;
    rfb.addEventListener("connect", () => setStatus(""));
    rfb.addEventListener("disconnect", () => setStatus("disconnected"));
    return () => rfb.disconnect();
  }, [sessionId]);

  return (
    <div className="flex min-h-96 flex-1 flex-col gap-2">
      {status && <p className="text-muted-foreground text-sm">{status}</p>}
      <div className="flex-1 overflow-hidden rounded-lg bg-black" ref={target} />
    </div>
  );
}

function LogPane({ sessionId, available }: { sessionId: string; available: boolean }) {
  const [text, setText] = useState<string>("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    const load = () =>
      fetchLog(sessionId)
        .then((t) => {
          if (live) {
            setText(t);
            setError(null);
          }
        })
        .catch((e) => live && setError(String(e)));
    load();
    const timer = setInterval(load, 2000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, [sessionId]);

  if (!available && !text) {
    return (
      <p className="text-muted-foreground py-12 text-center text-sm">
        {error
          ? "No log file for this session (enable with --save-all-logs or enableLog)."
          : "Waiting for log output…"}
      </p>
    );
  }
  return (
    <pre className="bg-card min-h-96 flex-1 overflow-auto rounded-lg border p-3 font-mono text-xs break-all whitespace-pre-wrap">
      {text || "…"}
    </pre>
  );
}
