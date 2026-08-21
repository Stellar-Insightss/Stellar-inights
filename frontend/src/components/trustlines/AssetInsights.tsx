import { Lightbulb } from "lucide-react";
import { InsightsList } from "@/components/dashboard/InsightsList";
import { AssetInsights as AssetInsightsType } from "@/lib/trustline-api";

function fmt(n: number) {
  return new Intl.NumberFormat("en-US", {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(n);
}

function buildSentences(data: AssetInsightsType): string[] {
  const sentences: string[] = [];
  const { largest_holder_pct, holder_growth_30d, transfer_volume_30d, avg_transfer_volume, total_trustlines } = data;

  if (largest_holder_pct !== null) {
    sentences.push(
      `The largest single holder controls ${largest_holder_pct.toFixed(1)}% of supply${largest_holder_pct > 50 ? ", indicating high concentration" : largest_holder_pct > 20 ? ", showing moderate concentration" : ", suggesting broad distribution"}.`
    );
  }

  if (holder_growth_30d !== null) {
    if (total_trustlines < 50) {
      sentences.push(
        `Trustline count changed by ${holder_growth_30d > 0 ? "+" : ""}${holder_growth_30d} over 30 days — sample size is small, so trends may not be representative.`
      );
    } else if (holder_growth_30d > 0) {
      sentences.push(`Holder count grew by ${holder_growth_30d} over the past 30 days.`);
    } else if (holder_growth_30d < 0) {
      sentences.push(`Holder count declined by ${Math.abs(holder_growth_30d)} over the past 30 days.`);
    } else {
      sentences.push(`Holder count was flat over the past 30 days.`);
    }
  }

  if (transfer_volume_30d !== null && avg_transfer_volume !== null && avg_transfer_volume > 0) {
    const ratio = transfer_volume_30d / avg_transfer_volume;
    if (ratio > 1.2) {
      sentences.push(`30-day transfer volume (${fmt(transfer_volume_30d)}) is ${((ratio - 1) * 100).toFixed(0)}% above the historical average.`);
    } else if (ratio < 0.8) {
      sentences.push(`30-day transfer volume (${fmt(transfer_volume_30d)}) is ${((1 - ratio) * 100).toFixed(0)}% below the historical average.`);
    } else {
      sentences.push(`30-day transfer volume (${fmt(transfer_volume_30d)}) is in line with the historical average.`);
    }
  } else if (transfer_volume_30d !== null) {
    sentences.push(`30-day transfer volume: ${fmt(transfer_volume_30d)}.`);
  }

  return sentences;
}

interface Props {
  data: AssetInsightsType | null;
  loading: boolean;
}

export function AssetInsights({ data, loading }: Props) {
  const insights = data ? buildSentences(data) : [];

  return (
    <div className="glass-card rounded-2xl p-6 border border-border/50">
      <div className="flex items-center gap-2 mb-4">
        <Lightbulb className="w-4 h-4 text-accent" />
        <h3 className="text-sm font-bold uppercase tracking-wider">Asset Insights</h3>
      </div>

      <InsightsList
        insights={insights}
        isLoading={loading}
        emptyMessage="Insufficient data to generate insights for this asset."
      />
    </div>
  );
}
