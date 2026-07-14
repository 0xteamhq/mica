import type { Capacity } from "../api";
import { Card } from "@/components/ui/card";

export function CapacityTiles({ capacity }: { capacity: Capacity | null }) {
  const tiles: Array<{ label: string; value: number | string }> = [
    { label: "Capacity", value: capacity?.total ?? "–" },
    { label: "In use", value: capacity?.used ?? "–" },
    { label: "Pending", value: capacity?.pending ?? "–" },
    { label: "Queued", value: capacity?.queued ?? "–" },
  ];
  return (
    <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
      {tiles.map((t) => (
        <Card key={t.label} className="gap-1 py-4">
          <div className="px-4 text-3xl font-semibold tabular-nums">{t.value}</div>
          <div className="text-muted-foreground px-4 text-xs font-medium tracking-wide uppercase">
            {t.label}
          </div>
        </Card>
      ))}
    </div>
  );
}
