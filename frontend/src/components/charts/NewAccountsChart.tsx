"use client";

import { useRef } from "react";
import {
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { ChartExportButton } from "./ChartExportButton";
import { getTooltipContentStyle } from "@/lib/chart-utils";
import type { NetworkNewAccountsPoint } from "@/lib/network-api";

interface NewAccountsChartProps {
  data: NetworkNewAccountsPoint[];
  loading?: boolean;
}

function formatCompact(value: number): string {
  return new Intl.NumberFormat("en-US", {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(value);
}

export function NewAccountsChart({
  data,
  loading = false,
}: NewAccountsChartProps) {
  const chartRef = useRef<HTMLDivElement>(null);

  const chartData = data.map((point) => ({
    label: new Date(point.date).toLocaleDateString("en-US", {
      month: "short",
      day: "numeric",
    }),
    date: point.date,
    count: point.count,
  }));

  const total = chartData.reduce((sum, d) => sum + d.count, 0);
  const latest = chartData[chartData.length - 1]?.count ?? 0;
  const peak = chartData.length
    ? Math.max(...chartData.map((d) => d.count))
    : 0;

  if (loading) {
    return (
      <div
        className="glass-card rounded-2xl p-6 border border-border/50 h-[420px] animate-pulse"
        aria-busy="true"
        aria-label="Loading new accounts"
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
        aria-labelledby="new-accounts-heading"
        className="glass-card rounded-2xl p-6 border border-border/50 flex flex-col items-center justify-center h-[420px]"
      >
        <div className="text-[10px] font-mono text-accent uppercase tracking-[0.2em] mb-2">
          Network // New Accounts
        </div>
        <h2
          id="new-accounts-heading"
          className="text-xl font-black tracking-tighter uppercase italic mb-2 opacity-50"
        >
          New Accounts
        </h2>
        <p className="text-sm font-mono text-muted-foreground uppercase tracking-widest text-center max-w-md">
          No new-account series yet. Data appears once{" "}
          <code className="text-accent">/api/v1/network/new-accounts</code>{" "}
          returns daily account-creation counts.
        </p>
      </section>
    );
  }

  return (
    <section
      ref={chartRef}
      aria-labelledby="new-accounts-heading"
      className="glass-card rounded-2xl p-6 border border-border/50"
    >
      <div className="flex flex-col md:flex-row md:items-start justify-between mb-8 gap-4">
        <div className="flex-1">
          <div className="text-[10px] font-mono text-accent uppercase tracking-[0.2em] mb-2">
            Network // New Accounts
          </div>
          <h2
            id="new-accounts-heading"
            className="text-xl font-black tracking-tighter uppercase italic mb-2"
          >
            New Accounts
          </h2>
          <p className="text-[10px] font-mono text-muted-foreground uppercase tracking-widest">
            Accounts created per day
          </p>
        </div>
        <ChartExportButton chartRef={chartRef} chartName="New Accounts" />
      </div>

      <div className="grid grid-cols-3 gap-4 mb-8">
        <div className="p-3 rounded-xl bg-slate-900/30 border border-white/5">
          <p className="text-[9px] font-mono text-muted-foreground uppercase tracking-wider mb-1">
            Latest day
          </p>
          <p className="text-xl font-black font-mono tracking-tighter text-emerald-400">
            {formatCompact(latest)}
          </p>
        </div>
        <div className="p-3 rounded-xl bg-slate-900/30 border border-white/5">
          <p className="text-[9px] font-mono text-muted-foreground uppercase tracking-wider mb-1">
            Period total
          </p>
          <p className="text-xl font-black font-mono tracking-tighter text-foreground/80">
            {formatCompact(total)}
          </p>
        </div>
        <div className="p-3 rounded-xl bg-slate-900/30 border border-white/5">
          <p className="text-[9px] font-mono text-muted-foreground uppercase tracking-wider mb-1">
            Peak day
          </p>
          <p className="text-xl font-black font-mono tracking-tighter text-accent">
            {formatCompact(peak)}
          </p>
        </div>
      </div>

      <div className="h-[300px] w-full">
        <ResponsiveContainer width="100%" height="100%">
          <BarChart
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
              tickFormatter={formatCompact}
              tick={{ fontSize: 10, fontFamily: "monospace" }}
              axisLine={false}
              tickLine={false}
              dx={-10}
              width={48}
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
                formatCompact(typeof value === "number" ? value : Number(value)),
                "New accounts",
              ]}
            />
            <Bar
              dataKey="count"
              name="New accounts"
              fill="#f59e0b"
              radius={[4, 4, 0, 0]}
            />
          </BarChart>
        </ResponsiveContainer>
      </div>
    </section>
  );
}
