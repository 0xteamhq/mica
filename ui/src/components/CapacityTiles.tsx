import { Activity, Hourglass, ListOrdered, Server } from "lucide-react";
import type { Capacity } from "../api";
import { Card } from "@/components/ui/card";

export function CapacityTiles({ capacity }: { capacity: Capacity | null }) {
  const tiles: Array<{ label: string; value: number | string; icon: typeof Server }> = [
    { label: "Capacity", value: capacity?.total ?? "–", icon: Server },
    { label: "In use", value: capacity?.used ?? "–", icon: Activity },
    { label: "Pending", value: capacity?.pending ?? "–", icon: Hourglass },
    { label: "Queued", value: capacity?.queued ?? "–", icon: ListOrdered },
  ];
  return (
    <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
      {tiles.map(({ label, value, icon: Icon }) => (
        <Card key={label} className="py-0">
          <div className="flex items-center justify-between gap-3 p-5">
            <div className="flex min-w-0 flex-col gap-1">
              <span className="text-muted-foreground text-xs font-medium tracking-wide uppercase">
                {label}
              </span>
              <span className="text-3xl font-semibold tabular-nums">{value}</span>
            </div>
            <div className="bg-muted/60 text-muted-foreground flex size-11 shrink-0 items-center justify-center rounded-xl">
              <Icon className="size-5" />
            </div>
          </div>
        </Card>
      ))}
    </div>
  );
}
