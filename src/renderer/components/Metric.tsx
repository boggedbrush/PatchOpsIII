import type { ReactNode } from "react";

type MetricProps = {
  label: string;
  value: ReactNode;
  tone?: "amber" | "green" | "red" | "steel";
};

export function Metric({ label, value, tone = "steel" }: MetricProps) {
  return (
    <div className={`metric metric-${tone}`}>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}
