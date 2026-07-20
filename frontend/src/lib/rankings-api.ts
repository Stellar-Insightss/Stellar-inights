/**
 * Asset Rankings API Client
 * Fetches top Stellar assets ranked by holder count and by trading volume.
 * Backed by `GET /api/v1/rankings/assets` (Stellar-Insightss/backend#39).
 */
import { logger } from "@/lib/logger";
import { config } from "@/config";

const API_BASE = config.apiUrl;

export type RankingSortBy = "holders" | "volume";

export interface AssetRanking {
  rank: number;
  asset_code: string;
  asset_issuer: string;
  holder_count: number;
  volume_24h_usd: number;
}

async function safeFetch<T>(url: string, fallback: T): Promise<T> {
  try {
    const response = await fetch(url, {
      method: "GET",
      headers: { "Content-Type": "application/json" },
    });
    if (!response.ok) throw new Error(`API error: ${response.status}`);
    return response.json();
  } catch (error) {
    const isNetworkError =
      error instanceof TypeError &&
      (error.message.includes("Failed to fetch") ||
        error.message.includes("Network request failed"));
    if (!isNetworkError) {
      logger.error(`Failed to fetch ${url}:`, error);
    }
    return fallback;
  }
}

/**
 * Fetch top assets sorted by holder count or by 24h trading volume.
 * Falls back to representative mock rankings if the backend endpoint
 * (backend#39) is unreachable, so the page never renders broken.
 */
export async function fetchAssetRankings(
  sortBy: RankingSortBy = "holders",
  limit: number = 50,
): Promise<AssetRanking[]> {
  return safeFetch(
    `${API_BASE}/api/v1/rankings/assets?sort_by=${sortBy}&limit=${limit}`,
    getMockRankings(sortBy),
  );
}

// =============================================================================
// Mock Data
// =============================================================================

const MOCK_ASSETS: Omit<AssetRanking, "rank">[] = [
  {
    asset_code: "XLM",
    asset_issuer: "native",
    holder_count: 6_800_000,
    volume_24h_usd: 182_500_000,
  },
  {
    asset_code: "USDC",
    asset_issuer:
      "GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN",
    holder_count: 412_000,
    volume_24h_usd: 96_300_000,
  },
  {
    asset_code: "AQUA",
    asset_issuer:
      "GBNZILSTVQZ4R7IKQDGHYGY2QXL5QOFJYQMXPKWRRM5PAV7Y4M67AQUA",
    holder_count: 118_500,
    volume_24h_usd: 4_120_000,
  },
  {
    asset_code: "yXLM",
    asset_issuer:
      "GARDNV3Q7YGT4AKSDF25LT32YSCCW4EV22Y2TV3I2PU2MMXJTEDL5T55",
    // Fewer holders than AQUA but higher trade volume, so the two sort
    // modes produce a genuinely different order (demonstrates the toggle).
    holder_count: 41_200,
    volume_24h_usd: 5_450_000,
  },
];

function getMockRankings(sortBy: RankingSortBy): AssetRanking[] {
  const sorted = [...MOCK_ASSETS].sort((a, b) =>
    sortBy === "holders"
      ? b.holder_count - a.holder_count
      : b.volume_24h_usd - a.volume_24h_usd,
  );

  return sorted.map((asset, index) => ({ ...asset, rank: index + 1 }));
}
