"use client";

import React, { useEffect, useState } from "react";
import dynamic from "next/dynamic";
import { Activity, Share2, Info } from "lucide-react";
import {
  fetchNetworkPaymentVolume,
  fetchNetworkNewAccounts,
  type NetworkPaymentVolumePoint,
  type NetworkNewAccountsPoint,
} from "@/lib/network-api";
import { logger } from "@/lib/logger";

const NetworkGraph = dynamic(
  () =>
    import("@/components/charts/NetworkGraph").then((m) => ({
      default: m.NetworkGraph,
    })),
  { ssr: false },
);

const PaymentVolumeChart = dynamic(
  () =>
    import("@/components/charts/PaymentVolumeChart").then((m) => ({
      default: m.PaymentVolumeChart,
    })),
  {
    ssr: false,
    loading: () => (
      <div className="glass-card rounded-2xl p-6 border border-border/50 h-[420px] animate-pulse">
        <div className="h-4 w-40 bg-white/5 rounded mb-4" />
        <div className="h-8 w-64 bg-white/5 rounded mb-8" />
        <div className="h-[260px] w-full bg-white/5 rounded-xl" />
      </div>
    ),
  },
);

const NewAccountsChart = dynamic(
  () =>
    import("@/components/charts/NewAccountsChart").then((m) => ({
      default: m.NewAccountsChart,
    })),
  {
    ssr: false,
    loading: () => (
      <div className="glass-card rounded-2xl p-6 border border-border/50 h-[420px] animate-pulse">
        <div className="h-4 w-40 bg-white/5 rounded mb-4" />
        <div className="h-8 w-64 bg-white/5 rounded mb-8" />
        <div className="h-[260px] w-full bg-white/5 rounded-xl" />
      </div>
    ),
  },
);

export default function NetworkPage() {
  const [data, setData] = useState<any>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [volumePoints, setVolumePoints] = useState<NetworkPaymentVolumePoint[]>(
    [],
  );
  const [volumeLoading, setVolumeLoading] = useState(true);
  const [newAccountsPoints, setNewAccountsPoints] = useState<
    NetworkNewAccountsPoint[]
  >([]);
  const [newAccountsLoading, setNewAccountsLoading] = useState(true);

  useEffect(() => {
    async function fetchGraphData() {
      try {
        const res = await fetch("/api/network-graph");
        if (!res.ok) throw new Error("Failed to fetch graph data");
        const json = await res.json();
        setData(json);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Unknown error");
      } finally {
        setLoading(false);
      }
    }
    void fetchGraphData();
  }, []);

  useEffect(() => {
    async function loadPaymentVolume() {
      setVolumeLoading(true);
      try {
        const result = await fetchNetworkPaymentVolume(30);
        setVolumePoints(result.points);
      } catch (err) {
        logger.error("Failed to load payment volume panel:", err);
        setVolumePoints([]);
      } finally {
        setVolumeLoading(false);
      }
    }
    void loadPaymentVolume();
  }, []);

  useEffect(() => {
    async function loadNewAccounts() {
      setNewAccountsLoading(true);
      try {
        const result = await fetchNetworkNewAccounts(30);
        setNewAccountsPoints(result.points);
      } catch (err) {
        logger.error("Failed to load new accounts panel:", err);
        setNewAccountsPoints([]);
      } finally {
        setNewAccountsLoading(false);
      }
    }
    void loadNewAccounts();
  }, []);

  return (
    <div className="min-h-screen p-8 lg:p-12 space-y-10">
      {/* Header Area */}
      <div className="flex flex-col lg:flex-row lg:items-center justify-between gap-6">
        <div>
          <div className="flex items-center gap-3 mb-2">
            <div className="p-2 bg-accent/20 rounded-lg">
              <Share2 className="w-5 h-5 text-accent" />
            </div>
            <h1 className="text-4xl font-bold tracking-tighter">
              Network Dashboard
            </h1>
          </div>
          <p className="text-muted-foreground text-sm max-w-xl">
            Payment volume, topology, and corridor relationships across Stellar
            anchors and assets.
          </p>
        </div>

        <div className="flex items-center gap-4">
          <div className="glass px-6 py-4 rounded-2xl flex items-center gap-4 border border-white/5">
            <div className="text-right">
              <div className="text-[10px] font-bold text-muted-foreground uppercase tracking-widest">
                Global Graph
              </div>
              <div className="text-xl font-bold tabular-nums">
                {data ? `${data.nodes.length} Nodes` : "--"}
              </div>
            </div>
            <div className="w-px h-8 bg-white/10" />
            <div className="text-right">
              <div className="text-[10px] font-bold text-muted-foreground uppercase tracking-widest">
                Active Links
              </div>
              <div className="text-xl font-bold tabular-nums">
                {data ? `${data.links.length} Edges` : "--"}
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Network metrics panels */}
      <div className="grid grid-cols-1 xl:grid-cols-2 gap-6">
        <NewAccountsChart
          data={newAccountsPoints}
          loading={newAccountsLoading}
        />
        <PaymentVolumeChart data={volumePoints} loading={volumeLoading} />
      </div>

      {/* Topology */}
      <div className="relative h-[calc(100vh-320px)] min-h-[500px]">
        {loading ? (
          <div className="w-full h-full glass rounded-3xl flex flex-col items-center justify-center gap-4">
            <div className="w-12 h-12 border-4 border-accent/20 border-t-accent rounded-full animate-spin" />
            <p className="text-sm font-bold uppercase tracking-widest text-muted-foreground animate-pulse">
              Calculating Graph Layout...
            </p>
          </div>
        ) : error ? (
          <div className="w-full h-full glass rounded-3xl flex flex-col items-center justify-center gap-4 text-red-400">
            <Activity className="w-12 h-12" />
            <p className="font-bold uppercase tracking-widest">
              Telemetry Data Unavailable
            </p>
            <p className="text-sm text-muted-foreground">{error}</p>
          </div>
        ) : data ? (
          <NetworkGraph data={data} />
        ) : (
          <div className="w-full h-full glass rounded-3xl flex flex-col items-center justify-center gap-4 text-muted-foreground">
            <p className="font-bold uppercase tracking-widest text-sm">
              No graph data
            </p>
          </div>
        )}

        <div className="mt-8 grid grid-cols-1 md:grid-cols-3 gap-6">
          <div className="p-6 glass border border-white/5 rounded-2xl">
            <div className="flex items-center gap-2 mb-4 text-accent">
              <Info className="w-4 h-4" />
              <h3 className="font-bold text-[10px] uppercase tracking-widest">
                Clusters
              </h3>
            </div>
            <p className="text-xs text-muted-foreground leading-relaxed">
              Nodes that gravitate together represent a shared ecosystem. Larger
              nodes indicate high-volume anchors or assets with multiple active
              trustlines.
            </p>
          </div>
          <div className="p-6 glass border border-white/5 rounded-2xl">
            <div className="flex items-center gap-2 mb-4 text-amber-400">
              <Activity className="w-4 h-4" />
              <h3 className="font-bold text-[10px] uppercase tracking-widest">
                Corridors
              </h3>
            </div>
            <p className="text-xs text-muted-foreground leading-relaxed">
              Lines between assets represent corridors. Thickness is proportional
              to USD liquidity depth, while color reflects recent payment success
              rates.
            </p>
          </div>
          <div className="p-6 glass border border-white/5 rounded-2xl">
            <div className="flex items-center gap-2 mb-4 text-green-400">
              <Share2 className="w-4 h-4" />
              <h3 className="font-bold text-[10px] uppercase tracking-widest">
                Navigation
              </h3>
            </div>
            <p className="text-xs text-muted-foreground leading-relaxed">
              Drag nodes to reorganize the layout. Use the mouse wheel to zoom.
              Hover over any element to view detailed performance metrics.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
