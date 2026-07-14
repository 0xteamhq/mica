import { Star, X } from "lucide-react";
import type { BrowserInfo } from "../api";
import { BROWSER_ACCENT } from "@/lib/browsers";
import { cn } from "@/lib/utils";
import { Card } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";

export function RegistryView({
  browsers,
  onSetDefault,
  onRemove,
}: {
  browsers: Record<string, BrowserInfo>;
  /** When provided, each non-default version shows a "set default" action. */
  onSetDefault?: (browser: string, version: string) => void;
  /** When provided, each version shows a "remove" action. */
  onRemove?: (browser: string, version: string) => void;
}) {
  const names = Object.keys(browsers).sort();
  if (names.length === 0) {
    return (
      <Card className="items-center gap-1 py-12 text-center">
        <p className="text-sm font-medium">No browsers configured</p>
        <p className="text-muted-foreground text-sm">Add one above to get started.</p>
      </Card>
    );
  }
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      {names.map((name) => {
        const info = browsers[name];
        const accent = BROWSER_ACCENT[name.toLowerCase()] ?? "var(--muted-foreground)";
        return (
          <Card key={name} className="gap-0 overflow-hidden py-0">
            <div className="flex items-center gap-3 p-4">
              <div
                className="flex size-9 shrink-0 items-center justify-center rounded-lg text-sm font-bold text-white"
                style={{ backgroundColor: accent }}
              >
                {name.charAt(0).toUpperCase()}
              </div>
              <div className="min-w-0">
                <div className="font-medium capitalize">{name}</div>
                <div className="text-muted-foreground text-xs">
                  {info.versions.length} version{info.versions.length === 1 ? "" : "s"}
                </div>
              </div>
            </div>
            <Separator />
            <div className="flex flex-wrap gap-1.5 p-4">
              {info.versions.map((v) => {
                const isDefault = v === info.default;
                return (
                  <span
                    key={v}
                    className={cn(
                      "group inline-flex items-center gap-1 rounded-md border py-1 pr-1 pl-2 text-xs",
                      isDefault
                        ? "border-primary/40 bg-primary/10 text-primary font-medium"
                        : "text-foreground",
                    )}
                  >
                    {v}
                    {isDefault && (
                      <span className="ml-0.5 text-[10px] tracking-wide uppercase opacity-70">
                        default
                      </span>
                    )}
                    {onSetDefault && !isDefault && (
                      <button
                        type="button"
                        title="Set as default"
                        onClick={() => onSetDefault(name, v)}
                        className="text-muted-foreground hover:bg-accent hover:text-foreground rounded p-0.5 transition-colors"
                      >
                        <Star className="size-3" />
                      </button>
                    )}
                    {onRemove && (
                      <button
                        type="button"
                        title="Remove version"
                        onClick={() => onRemove(name, v)}
                        className="text-muted-foreground hover:bg-destructive/10 hover:text-destructive rounded p-0.5 transition-colors"
                      >
                        <X className="size-3" />
                      </button>
                    )}
                  </span>
                );
              })}
            </div>
          </Card>
        );
      })}
    </div>
  );
}
