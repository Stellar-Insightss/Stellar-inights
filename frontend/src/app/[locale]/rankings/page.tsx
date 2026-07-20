"use client";

import React, { useEffect, useState } from "react";
import { Award, BadgeCheck, Trophy, TrendingUp, Users } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import {
  AssetRanking,
  RankingSortBy,
  fetchAssetRankings,
} from "@/lib/rankings-api";

const SORT_MODES: {
  key: RankingSortBy;
  label: string;
  icon: typeof Users;
}[] = [
  { key: "holders", label: "By Holders", icon: Users },
  { key: "volume", label: "By Volume", icon: TrendingUp },
];

export default function RankingsPage() {
  const [sortBy, setSortBy] = useState<RankingSortBy>("holders");
  const [rankings, setRankings] = useState<AssetRanking[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    async function loadRankings() {
      setLoading(true);
      const data = await fetchAssetRankings(sortBy);
      setRankings(data);
      setLoading(false);
    }
    loadRankings();
  }, [sortBy]);

  const formatNumber = (value: number) =>
    new Intl.NumberFormat("en-US", {
      notation: "compact",
      maximumFractionDigits: 1,
    }).format(value);

  const formatCurrency = (value: number) =>
    new Intl.NumberFormat("en-US", {
      style: "currency",
      currency: "USD",
      notation: "compact",
      maximumFractionDigits: 1,
    }).format(value);

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 pt-4 sm:pt-6 space-y-6 sm:space-y-8 animate-in fade-in duration-700 pb-12">
      {/* Header */}
      <div className="flex flex-col gap-2">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 bg-accent/20 rounded-xl flex items-center justify-center border border-accent/50 glow-accent shrink-0">
            <Trophy className="w-5 h-5 text-accent" />
          </div>
          <h1 className="text-3xl md:text-4xl font-black tracking-tighter uppercase italic">
            Asset
            <span className="text-accent underline decoration-accent/30 decoration-4 underline-offset-4 ml-2">
              Rankings
            </span>
          </h1>
        </div>
        <p className="text-xs sm:text-sm font-mono text-muted-foreground uppercase tracking-widest mt-2 md:mt-0 pl-1 md:pl-14">
          Top Stellar assets ranked by holder count and 24h trading volume
        </p>
      </div>

      {/* Sort Mode Tabs */}
      <div
        className="flex items-center gap-2"
        role="tablist"
        aria-label="Ranking sort mode"
      >
        {SORT_MODES.map(({ key, label, icon: Icon }) => {
          const isActive = sortBy === key;
          return (
            <button
              key={key}
              type="button"
              role="tab"
              aria-selected={isActive}
              onClick={() => setSortBy(key)}
              className={`flex items-center gap-2 px-4 py-2 rounded-xl text-[10px] font-mono font-bold uppercase tracking-widest transition-all duration-200 border ${
                isActive
                  ? "bg-accent/20 border-accent/50 text-accent"
                  : "bg-transparent border-border/30 text-muted-foreground hover:border-accent/30 hover:text-foreground"
              }`}
            >
              <Icon className="w-3.5 h-3.5" aria-hidden="true" />
              {label}
            </button>
          );
        })}
      </div>

      {/* Rankings Table */}
      <div className="glass-card rounded-2xl border border-border/50 overflow-hidden">
        <div className="p-4 border-b border-white/5 bg-slate-900/50 flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Award className="w-4 h-4 text-accent" aria-hidden="true" />
            <h2 className="text-sm font-bold uppercase tracking-wider">
              Top Assets {sortBy === "holders" ? "by Holders" : "by Volume"}
            </h2>
          </div>
          <Badge
            variant="outline"
            className="text-[10px] font-mono border-border/50"
          >
            {rankings.length} ASSETS
          </Badge>
        </div>

        {loading ? (
          <div className="flex items-center justify-center py-16">
            <div
              className="w-10 h-10 border-4 border-accent/20 border-t-accent rounded-full animate-spin"
              role="status"
              aria-label="Loading rankings"
            />
          </div>
        ) : rankings.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-16 gap-3">
            <Trophy className="w-10 h-10 text-muted-foreground/30" aria-hidden="true" />
            <p className="text-xs font-mono text-muted-foreground uppercase tracking-widest text-center">
              No ranking data available yet
            </p>
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-xs font-mono">
              <thead>
                <tr className="border-b border-border/30">
                  <th className="text-left px-6 py-3 text-[10px] uppercase tracking-widest text-muted-foreground font-bold">
                    Rank
                  </th>
                  <th className="text-left px-6 py-3 text-[10px] uppercase tracking-widest text-muted-foreground font-bold">
                    Asset
                  </th>
                  <th className="text-right px-6 py-3 text-[10px] uppercase tracking-widest text-muted-foreground font-bold">
                    Holders
                  </th>
                  <th className="text-right px-6 py-3 text-[10px] uppercase tracking-widest text-muted-foreground font-bold">
                    24H Volume
                  </th>
                </tr>
              </thead>
              <tbody>
                {rankings.map((asset) => (
                  <tr
                    key={`${asset.asset_code}-${asset.asset_issuer}`}
                    className="border-b border-border/10 transition-colors hover:bg-accent/5"
                  >
                    <td className="px-6 py-4 text-muted-foreground/50">
                      #{asset.rank}
                    </td>
                    <td className="px-6 py-4">
                      <div className="flex items-center gap-2">
                        <div className="w-7 h-7 rounded-full bg-gradient-to-br from-accent/30 to-accent/10 flex items-center justify-center text-[9px] font-black text-accent shrink-0">
                          {asset.asset_code.charAt(0)}
                        </div>
                        <div>
                          <div className="font-bold text-foreground text-xs flex items-center gap-1">
                            {asset.asset_code}
                            {asset.rank <= 10 && (
                              <BadgeCheck
                                className="w-3 h-3 text-emerald-400"
                                aria-hidden="true"
                              />
                            )}
                          </div>
                          <div className="text-[9px] text-muted-foreground/50">
                            {asset.asset_issuer === "native"
                              ? "Native Asset"
                              : `${asset.asset_issuer.slice(0, 4)}...${asset.asset_issuer.slice(-4)}`}
                          </div>
                        </div>
                      </div>
                    </td>
                    <td className="px-6 py-4 text-right font-bold">
                      {formatNumber(asset.holder_count)}
                    </td>
                    <td className="px-6 py-4 text-right font-bold text-emerald-400">
                      {formatCurrency(asset.volume_24h_usd)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}
