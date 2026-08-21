"use client";

import { useRef } from "react";
import {
  Line,
  LineChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { ChartExportButton } from "./ChartExportButton";
import { getTooltipContentStyle } from "@/lib/chart-utils";
import type { NetworkFeeTrendPoint } from "@/lib/network-api";

interface FeeTrendsChartProps {
  data: NetworkFeeTrendPoint[];
  loading?: boolean;
}

function formatStroops(value: number): string {
  // 1 XLM = 10,000,000 stroops. Fees are typically sub-XLM, so show stroops
  // directly with thousands separators rather than converting to XLM.
  return new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(
    value,
  );
}

export function FeeTrendsChart({ data, loading = false }: FeeTrendsChartProps) {
  const chartRef = useRef<HTMLDivElement>(null);

  const chartData = data.map((point) => ({
    label: new Date(point.date).toLocaleDateString("en-US", {
      month: "short",
      day: "numeric",
    }),
    date: point.date,
    avgFeeStroops: point.avgFeeStroops,
  }));

  const latest = chartData[chartData.length - 1]?.avgFeeStroops ?? 0;
  const peak = chartData.length
    ? Math.max(...chartData.map((d) => d.avgFeeStroops))
    : 0;
  const average = chartData.length
    ? chartData.reduce((sum, d) => sum + d.avgFeeStroops, 0) / chartData.length
    : 0;

  if (loading) {
    return (
      <div
        className="glass-card rounded-2xl p-6 border border-border/50 h-[420px] animate-pulse"
        aria-busy="true"
        aria-label="Loading fee trends"
      >
        <div className="h-4 w-40 bg-white/5 rounded mb-4" />
        <div className="h-8 w-64 bg-white/5 rounded mb-8" />
        <div className="h-[260px] w-full bg-white/5 rounded-xl" />
      </div>
    );
  }

  if (chartData.length === 0) {
    return (
      <section
        aria-labelledby="fee-trends-heading"
        className="glass-card rounded-2xl p-6 border border-border/50 flex flex-col items-center justify-center h-[420px]"
      >
        <div className="text-[10px] font-mono text-accent uppercase tracking-[0.2em] mb-2">
          Network // Fee Trends
        </div>
        <h2
          id="fee-trends-heading"
          className="text-xl font-black tracking-tighter uppercase italic mb-2 opacity-50"
        >
          Fee Trends
        </h2>
        <p className="text-sm font-mono text-muted-foreground uppercase tracking-widest text-center max-w-md">
          No fee series yet. Data appears once{" "}
          <code className="text-accent">/api/v1/network/fee-trends</code>{" "}
          returns daily average transaction fees.
        </p>
      </section>
    );
  }

  return (
    <section
      ref={chartRef}
      aria-labelledby="fee-trends-heading"
      className="glass-card rounded-2xl p-6 border border-border/50"
    >
      <div className="flex flex-col md:flex-row md:items-start justify-between mb-8 gap-4">
        <div className="flex-1">
          <div className="text-[10px] font-mono text-accent uppercase tracking-[0.2em] mb-2">
            Network // Fee Trends
          </div>
          <h2
            id="fee-trends-heading"
            className="text-xl font-black tracking-tighter uppercase italic mb-2"
          >
            Fee Trends
          </h2>
          <p className="text-[10px] font-mono text-muted-foreground uppercase tracking-widest">
            Average transaction fee per day (stroops)
          </p>
        </div>
        <ChartExportButton chartRef={chartRef} chartName="Fee Trends" />
      </div>

      <div className="grid grid-cols-3 gap-4 mb-8">
        <div className="p-3 rounded-xl bg-slate-900/30 border border-white/5">
          <p className="text-[9px] font-mono text-muted-foreground uppercase tracking-wider mb-1">
            Latest day
          </p>
          <p className="text-xl font-black font-mono tracking-tighter text-emerald-400">
            {formatStroops(latest)}
          </p>
        </div>
        <div className="p-3 rounded-xl bg-slate-900/30 border border-white/5">
          <p className="text-[9px] font-mono text-muted-foreground uppercase tracking-wider mb-1">
            Period average
          </p>
          <p className="text-xl font-black font-mono tracking-tighter text-foreground/80">
            {formatStroops(average)}
          </p>
        </div>
        <div className="p-3 rounded-xl bg-slate-900/30 border border-white/5">
          <p className="text-[9px] font-mono text-muted-foreground uppercase tracking-wider mb-1">
            Peak day
          </p>
          <p className="text-xl font-black font-mono tracking-tighter text-accent">
            {formatStroops(peak)}
          </p>
        </div>
      </div>

      <div className="h-[300px] w-full">
        <ResponsiveContainer width="100%" height="100%">
          <LineChart
            data={chartData}
            margin={{ top: 10, right: 10, left: 0, bottom: 0 }}
          >
            <CartesianGrid
              strokeDasharray="3 3"
              stroke="rgba(255,255,255,0.05)"
              vertical={false}
            />
            <XAxis
              dataKey="label"
              stroke="rgba(255,255,255,0.3)"
              tick={{ fontSize: 10, fontFamily: "monospace" }}
              axisLine={false}
              tickLine={false}
              dy={10}
            />
            <YAxis
              stroke="rgba(255,255,255,0.3)"
              tickFormatter={formatStroops}
              tick={{ fontSize: 10, fontFamily: "monospace" }}
              axisLine={false}
              tickLine={false}
              dx={-10}
              width={56}
            />
            <Tooltip
              contentStyle={getTooltipContentStyle({
                backgroundColor: "rgba(15, 23, 42, 0.9)",
                border: "1px solid rgba(255, 255, 255, 0.1)",
                borderRadius: "12px",
                fontSize: "10px",
                fontFamily: "monospace",
              })}
              labelStyle={{ color: "#94a3b8", marginBottom: "4px" }}
              formatter={(value) => [
                `${formatStroops(typeof value === "number" ? value : Number(value))} stroops`,
                "Avg fee",
              ]}
            />
            <Line
              type="monotone"
              dataKey="avgFeeStroops"
              stroke="#f472b6"
              strokeWidth={2.5}
              dot={false}
              name="Avg fee"
              activeDot={{
                r: 4,
                fill: "#f472b6",
                stroke: "#fff",
                strokeWidth: 2,
              }}
            />
          </LineChart>
        </ResponsiveContainer>
      </div>
    </section>
  );
}
