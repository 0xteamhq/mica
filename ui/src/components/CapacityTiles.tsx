import type { Capacity } from "../api";

export function CapacityTiles({ capacity }: { capacity: Capacity | null }) {
  const tiles: Array<{ label: string; value: number | string }> = [
    { label: "capacity", value: capacity?.total ?? "–" },
    { label: "in use", value: capacity?.used ?? "–" },
    { label: "pending", value: capacity?.pending ?? "–" },
    { label: "queued", value: capacity?.queued ?? "–" },
  ];
  return (
    <div className="tiles">
      {tiles.map((t) => (
        <div className="tile" key={t.label}>
          <div className="tile-value">{t.value}</div>
          <div className="tile-label">{t.label}</div>
        </div>
      ))}
    </div>
  );
}
