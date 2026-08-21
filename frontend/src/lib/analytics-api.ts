/**
 * Analytics API Client
 * Handles all analytics data fetching from the backend
 */
import { logger } from "@/lib/logger";

export interface CorridorAnalytics {
  corridor_key: string;
  asset_a_code: string;
  asset_a_issuer: string;
  asset_b_code: string;
  asset_b_issuer: string;
  success_rate: number;
  total_transactions: number;
  successful_transactions: number;
  failed_transactions: number;
  volume_usd: number;
  avg_settlement_latency_ms?: number;
  liquidity_depth_usd: number;
  date: string;
}

export interface LiquidityDataPoint {
  timestamp: string;
  liquidity_usd: number;
  corridor_key: string;
}

export interface TVLDataPoint {
  timestamp: string;
  tvl_usd: number;
}

export interface SettlementLatencyDataPoint {
  timestamp: string;
  median_latency_ms: number;
  p95_latency_ms: number;
  p99_latency_ms: number;
}

export interface BalanceHistoryDataPoint {
  timestamp: string;
  balance_usd: number;
  asset_code: string;
  asset_issuer?: string;
}

export interface WalletBalanceHistoryResponse {
  address: string;
  balance_history: BalanceHistoryDataPoint[];
}

export interface ActivityDay {
  date: string;
  count: number;
}

export interface WalletActivityCalendarResponse {
  address: string;
  activity: ActivityDay[];
}

export type TransferDirection = "in" | "out";

export interface LargestTransfer {
  id: string;
  transaction_hash: string;
  counterparty: string;
  direction: TransferDirection;
  amount: number;
  asset_code: string;
  asset_issuer?: string;
  amount_usd: number;
  timestamp: string;
}

export interface WalletLargestTransfersResponse {
  address: string;
  transfers: LargestTransfer[];
}

export interface AnalyticsMetrics {
  top_corridors: CorridorAnalytics[];
  liquidity_history: LiquidityDataPoint[];
  tvl_history: TVLDataPoint[];
  settlement_latency_history: SettlementLatencyDataPoint[];
  total_volume_usd: number;
  avg_success_rate: number;
  active_corridors: number;
}

export interface ApiUsageOverview {
  total_requests: number;
  avg_response_time_ms: number;
  error_rate: number;
  top_endpoints: {
    endpoint: string;
    method: string;
    count: number;
    avg_response_time_ms: number;
  }[];
  status_distribution: {
    status_code: number;
    count: number;
  }[];
}

import { config } from '@/config';
const API_BASE = config.apiUrl;

export async function fetchAnalyticsMetrics(): Promise<AnalyticsMetrics> {
  try {
    const response = await fetch(`${API_BASE}/api/analytics`, {
      method: "GET",
      headers: {
        "Content-Type": "application/json",
      },
    });

    if (!response.ok) {
      throw new Error(`API error: ${response.status}`);
    }

    return response.json();
  } catch (error) {
    // Check if this is a network error (backend not running)
    const isNetworkError =
      error instanceof TypeError &&
      (error.message.includes("Failed to fetch") ||
        error.message.includes("fetch is not defined") ||
        error.message.includes("Network request failed"));

    // Only log non-network errors to avoid noise when backend is not running
    if (!isNetworkError) {
      logger.error("Failed to fetch analytics metrics:", error);
    }


    // Return mock data as fallback - this is expected when backend isn't running
    return getMockAnalyticsData();
  }
}

export async function fetchApiUsageOverview(): Promise<ApiUsageOverview> {
  try {
    const response = await fetch(`${API_BASE}/api/admin/analytics/overview`, {
      method: "GET",
      headers: {
        "Content-Type": "application/json",
      },
    });

    if (!response.ok) {
      throw new Error(`API error: ${response.status}`);
    }

    return response.json();
  } catch (error) {
    const isNetworkError = error instanceof TypeError &&
      (error.message.includes('Failed to fetch') ||
        error.message.includes('fetch is not defined') ||
        error.message.includes('Network request failed'));

    if (!isNetworkError) {
      logger.error("Failed to fetch API usage overview:", error instanceof Error ? error : new Error(String(error)));
    }

    return getMockApiUsageOverview();
  }
}

export function getMockApiUsageOverview(): ApiUsageOverview {
  return {
    total_requests: 15420,
    avg_response_time_ms: 42.5,
    error_rate: 1.2,
    top_endpoints: [
      { endpoint: "/api/anchors", method: "GET", count: 5400, avg_response_time_ms: 120.4 },
      { endpoint: "/api/corridors", method: "GET", count: 3200, avg_response_time_ms: 85.2 },
      { endpoint: "/api/prices", method: "GET", count: 2800, avg_response_time_ms: 45.1 },
      { endpoint: "/api/rpc/payments", method: "GET", count: 1500, avg_response_time_ms: 210.6 },
      { endpoint: "/health", method: "GET", count: 1200, avg_response_time_ms: 5.4 },
    ],
    status_distribution: [
      { status_code: 200, count: 14800 },
      { status_code: 201, count: 420 },
      { status_code: 404, count: 120 },
      { status_code: 500, count: 50 },
      { status_code: 401, count: 30 },
    ],
  };
}

// ─── Wallet dashboard types ────────────────────────────────────────────────

export interface WalletPortfolioSnapshot {
  address: string;
  total_value_usd: number;
  change_24h_usd: number;
  change_24h_pct: number;
  total_assets: number;
  total_transactions_30d: number;
  last_activity: string; // ISO timestamp
}

export interface WalletAssetAllocation {
  asset_code: string;
  asset_issuer?: string;
  balance: number;
  value_usd: number;
  pct_of_portfolio: number;
}

export interface WalletAllocationResponse {
  address: string;
  allocations: WalletAssetAllocation[];
}

export interface WalletActivityDay {
  date: string; // YYYY-MM-DD
  count: number; // number of transactions on that day
}

export interface WalletActivityResponse {
  address: string;
  activity: WalletActivityDay[];
}

export interface WalletTransfer {
  id: string;
  timestamp: string;
  direction: TransferDirection;
  asset_code: string;
  asset_issuer?: string;
  amount: number;
  amount_usd: number;
  counterparty: string; // Stellar address of the other party
  memo?: string;
  successful: boolean;
}

export interface WalletTransfersResponse {
  address: string;
  transfers: WalletTransfer[];
}

// ─── Wallet API fetchers ───────────────────────────────────────────────────

export async function fetchWalletPortfolio(address: string): Promise<WalletPortfolioSnapshot> {
  try {
    const response = await fetch(`${API_BASE}/api/wallets/${address}/portfolio`, {
      method: "GET",
      headers: { "Content-Type": "application/json" },
    });
    if (!response.ok) throw new Error(`API error: ${response.status}`);
    return response.json();
  } catch (error) {
    const isNetworkError =
      error instanceof TypeError &&
      (error.message.includes("Failed to fetch") ||
        error.message.includes("fetch is not defined") ||
        error.message.includes("Network request failed"));
    if (!isNetworkError) {
      logger.error("Failed to fetch wallet portfolio:", error instanceof Error ? error : new Error(String(error)));
    }
    return getMockWalletPortfolio(address);
  }
}

export async function fetchWalletAllocation(address: string): Promise<WalletAllocationResponse> {
  try {
    const response = await fetch(`${API_BASE}/api/wallets/${address}/allocation`, {
      method: "GET",
      headers: { "Content-Type": "application/json" },
    });
    if (!response.ok) throw new Error(`API error: ${response.status}`);
    return response.json();
  } catch (error) {
    const isNetworkError =
      error instanceof TypeError &&
      (error.message.includes("Failed to fetch") ||
        error.message.includes("fetch is not defined") ||
        error.message.includes("Network request failed"));
    if (!isNetworkError) {
      logger.error("Failed to fetch wallet allocation:", error instanceof Error ? error : new Error(String(error)));
    }
    return getMockWalletAllocation(address);
  }
}

export async function fetchWalletActivity(address: string): Promise<WalletActivityResponse> {
  try {
    const response = await fetch(`${API_BASE}/api/wallets/${address}/activity`, {
      method: "GET",
      headers: { "Content-Type": "application/json" },
    });
    if (!response.ok) throw new Error(`API error: ${response.status}`);
    return response.json();
  } catch (error) {
    const isNetworkError =
      error instanceof TypeError &&
      (error.message.includes("Failed to fetch") ||
        error.message.includes("fetch is not defined") ||
        error.message.includes("Network request failed"));
    if (!isNetworkError) {
      logger.error("Failed to fetch wallet activity:", error instanceof Error ? error : new Error(String(error)));
    }
    return getMockWalletActivity(address);
  }
}

export async function fetchWalletTransfers(address: string): Promise<WalletTransfersResponse> {
  try {
    const response = await fetch(`${API_BASE}/api/wallets/${address}/transfers`, {
      method: "GET",
      headers: { "Content-Type": "application/json" },
    });
    if (!response.ok) throw new Error(`API error: ${response.status}`);
    return response.json();
  } catch (error) {
    const isNetworkError =
      error instanceof TypeError &&
      (error.message.includes("Failed to fetch") ||
        error.message.includes("fetch is not defined") ||
        error.message.includes("Network request failed"));
    if (!isNetworkError) {
      logger.error("Failed to fetch wallet transfers:", error instanceof Error ? error : new Error(String(error)));
    }
    return getMockWalletTransfers(address);
  }
}

// ─── Mock data generators ──────────────────────────────────────────────────

export function getMockWalletPortfolio(address: string): WalletPortfolioSnapshot {
  const totalValue = 1800 + Math.random() * 600;
  const change24hUsd = (Math.random() - 0.4) * 120;
  return {
    address,
    total_value_usd: totalValue,
    change_24h_usd: change24hUsd,
    change_24h_pct: (change24hUsd / totalValue) * 100,
    total_assets: 3,
    total_transactions_30d: 14 + Math.floor(Math.random() * 20),
    last_activity: new Date(Date.now() - Math.random() * 86400000 * 3).toISOString(),
  };
}

export function getMockWalletAllocation(address: string): WalletAllocationResponse {
  const assets = [
    { asset_code: "XLM", asset_issuer: undefined, balance: 4200, value_usd: 630 },
    { asset_code: "USDC", asset_issuer: "GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN", balance: 900, value_usd: 900 },
    { asset_code: "yXLM", asset_issuer: "PAXOS_ISSUER_EXAMPLE_000000000000000000000000000000000000", balance: 200, value_usd: 270 },
  ];
  const total = assets.reduce((s, a) => s + a.value_usd, 0);
  return {
    address,
    allocations: assets.map((a) => ({
      ...a,
      pct_of_portfolio: (a.value_usd / total) * 100,
    })),
  };
}

export function getMockWalletActivity(address: string): WalletActivityResponse {
  const today = new Date();
  const activity: WalletActivityDay[] = [];
  for (let i = 364; i >= 0; i--) {
    const d = new Date(today);
    d.setDate(d.getDate() - i);
    const dateStr = d.toISOString().split("T")[0];
    // Sparse activity — most days are quiet
    const rand = Math.random();
    activity.push({
      date: dateStr,
      count: rand < 0.65 ? 0 : rand < 0.85 ? 1 : rand < 0.94 ? 2 : rand < 0.98 ? 4 : 7,
    });
  }
  return { address, activity };
}

export function getMockWalletTransfers(address: string): WalletTransfersResponse {
  const assets = ["XLM", "USDC", "yXLM"];
  const counterparties = [
    "GBUQWP3BOUZX34LOCALEXAMPLE1111111111111111111111111111A",
    "GDUVOLYZKFKDQM2CNXXXX2222222222222222222222222222222222",
    "GBPYHIL2CI3FNQ4BXLFMNDLFJUNPU2HY3ZMFSHONUCEOASW7QC7OX2H",
    "GCEZWKCA5VLDNRLN3RPRJMRZOX3Z6G5CHCGBK2KVT6XUJAUJ9K33Y94",
  ];

  const transfers: WalletTransfer[] = Array.from({ length: 20 }, (_, i) => {
    const direction: TransferDirection = Math.random() > 0.5 ? "in" : "out";
    const assetCode = assets[Math.floor(Math.random() * assets.length)];
    const amount = assetCode === "USDC"
      ? parseFloat((50 + Math.random() * 950).toFixed(2))
      : parseFloat((100 + Math.random() * 5000).toFixed(4));
    const amountUsd = assetCode === "USDC" ? amount : amount * 0.15;
    const daysAgo = i * 1.5;
    return {
      id: `tx_mock_${i}_${address.slice(0, 6)}`,
      timestamp: new Date(Date.now() - daysAgo * 86400000).toISOString(),
      direction,
      asset_code: assetCode,
      asset_issuer: assetCode !== "XLM" ? "GA5ZSEJYB37JRC5AVCIA5MOP4RHTM335X2KGX3IHOJAPP5RE34K4KZVN" : undefined,
      amount,
      amount_usd: amountUsd,
      counterparty: counterparties[Math.floor(Math.random() * counterparties.length)],
      memo: Math.random() > 0.6 ? `payment-ref-${Math.floor(Math.random() * 9999)}` : undefined,
      successful: Math.random() > 0.05,
    };
  });

  // Sort by most recent first, then take the largest 10 by USD value
  const sorted = [...transfers].sort((a, b) => b.amount_usd - a.amount_usd).slice(0, 10);

  return { address, transfers: sorted };
}

export async function fetchWalletBalanceHistory(address: string): Promise<WalletBalanceHistoryResponse> {
  try {
    const response = await fetch(`${API_BASE}/api/wallets/${address}/balance-history`, {
      method: "GET",
      headers: {
        "Content-Type": "application/json",
      },
    });

    if (!response.ok) {
      throw new Error(`API error: ${response.status}`);
    }

    return response.json();
  } catch (error) {
    const isNetworkError = error instanceof TypeError &&
      (error.message.includes('Failed to fetch') ||
        error.message.includes('fetch is not defined') ||
        error.message.includes('Network request failed'));

    if (!isNetworkError) {
      logger.error("Failed to fetch wallet balance history:", error instanceof Error ? error : new Error(String(error)));
    }

    return getMockWalletBalanceHistory(address);
  }
}

export function getMockWalletBalanceHistory(address: string): WalletBalanceHistoryResponse {
  const now = new Date();
  const lastSevenDays = Array.from({ length: 7 }, (_, i) => {
    const date = new Date(now);
    date.setDate(date.getDate() - (6 - i));
    return date;
  });

  const assets = [
    { asset_code: "XLM", asset_issuer: undefined },
    { asset_code: "USDC", asset_issuer: "GBUQWP3BOUZX34LOCALEXAMPLE" },
  ];

  const balanceHistory: BalanceHistoryDataPoint[] = lastSevenDays.flatMap((date) =>
    assets.map((asset) => ({
      timestamp: date.toISOString(),
      balance_usd: asset.asset_code === "XLM" 
        ? 500 + Math.random() * 200 + (date.getTime() - lastSevenDays[0].getTime()) * 0.001
        : 1200 + Math.random() * 400 + (date.getTime() - lastSevenDays[0].getTime()) * 0.003,
      asset_code: asset.asset_code,
      asset_issuer: asset.asset_issuer,
    }))
  );

  return {
    address,
    balance_history: balanceHistory,
  };
}

export async function fetchWalletActivityCalendar(
  address: string,
  days = 365,
): Promise<WalletActivityCalendarResponse> {
  try {
    const response = await fetch(
      `${API_BASE}/api/v1/wallets/${address}/activity-calendar?days=${days}`,
      {
        method: "GET",
        headers: {
          "Content-Type": "application/json",
        },
      },
    );

    if (!response.ok) {
      throw new Error(`API error: ${response.status}`);
    }

    return response.json();
  } catch (error) {
    const isNetworkError = error instanceof TypeError &&
      (error.message.includes('Failed to fetch') ||
        error.message.includes('fetch is not defined') ||
        error.message.includes('Network request failed'));

    if (!isNetworkError) {
      logger.error("Failed to fetch wallet activity calendar:", error instanceof Error ? error : new Error(String(error)));
    }

    return getMockWalletActivityCalendar(address, days);
  }
}

export function getMockWalletActivityCalendar(
  address: string,
  days = 365,
): WalletActivityCalendarResponse {
  const now = new Date();
  const activity: ActivityDay[] = Array.from({ length: days }, (_, i) => {
    const date = new Date(now);
    date.setDate(date.getDate() - (days - 1 - i));
    // Deterministic-ish pseudo-random activity so mock data looks organic
    // without relying on Math.random for every render.
    const seed = (date.getDate() * 31 + date.getMonth() * 7) % 11;
    const count = seed < 5 ? 0 : Math.min(seed - 4, 8);
    return {
      date: date.toISOString().slice(0, 10),
      count,
    };
  }).filter((d) => d.count > 0);

  return { address, activity };
}

export async function fetchWalletLargestTransfers(
  address: string,
  limit = 20,
): Promise<WalletLargestTransfersResponse> {
  try {
    const response = await fetch(
      `${API_BASE}/api/v1/wallets/${address}/largest-transfers?limit=${limit}`,
      {
        method: "GET",
        headers: {
          "Content-Type": "application/json",
        },
      },
    );

    if (!response.ok) {
      throw new Error(`API error: ${response.status}`);
    }

    return response.json();
  } catch (error) {
    const isNetworkError = error instanceof TypeError &&
      (error.message.includes('Failed to fetch') ||
        error.message.includes('fetch is not defined') ||
        error.message.includes('Network request failed'));

    if (!isNetworkError) {
      logger.error("Failed to fetch wallet largest transfers:", error instanceof Error ? error : new Error(String(error)));
    }

    return getMockWalletLargestTransfers(address, limit);
  }
}

export function getMockWalletLargestTransfers(
  address: string,
  limit = 20,
): WalletLargestTransfersResponse {
  const now = new Date();
  const assets: { code: string; issuer?: string; usdPerUnit: number }[] = [
    { code: "XLM", usdPerUnit: 0.12 },
    { code: "USDC", issuer: "GBUQWP3BOUZX34LOCALEXAMPLE", usdPerUnit: 1 },
    { code: "BTC", issuer: "GDXTJEK4JZNSTNQAWA53RZNS2GIKTDRPEUWDXELFMKU52XNECNVDVXDI", usdPerUnit: 62000 },
  ];

  const transfers: LargestTransfer[] = Array.from({ length: limit }, (_, i) => {
    const asset = assets[i % assets.length];
    const amount = (limit - i) * (asset.code === "BTC" ? 0.05 : 1000);
    const date = new Date(now);
    date.setDate(date.getDate() - i);

    return {
      id: `mock-${i}`,
      transaction_hash: `mockhash${i}`,
      counterparty: `GMOCKCOUNTERPARTY${i.toString().padStart(2, "0")}EXAMPLE`,
      direction: i % 2 === 0 ? "out" : "in",
      amount,
      asset_code: asset.code,
      asset_issuer: asset.issuer,
      amount_usd: amount * asset.usdPerUnit,
      timestamp: date.toISOString(),
    };
  }).sort((a, b) => b.amount_usd - a.amount_usd);

  return { address, transfers };
}

export function getMockAnalyticsData(): AnalyticsMetrics {
  const now = new Date();
  const lastSevenDays = Array.from({ length: 7 }, (_, i) => {
    const date = new Date(now);
    date.setDate(date.getDate() - (6 - i));
    return date;
  });

  const corridors = [
    {
      corridor_key: "USDC:GBUQWP3BOUZX34LOCALEXAMPLE->PHP:PHPEXAMPLEISSUER",
      asset_a_code: "USDC",
      asset_a_issuer: "GBUQWP3BOUZX34LOCALEXAMPLE",
      asset_b_code: "PHP",
      asset_b_issuer: "PHPEXAMPLEISSUER",
      success_rate: 98.5,
      total_transactions: 2450,
      successful_transactions: 2411,
      failed_transactions: 39,
      volume_usd: 890000,
      avg_settlement_latency_ms: 2340,
      liquidity_depth_usd: 5600000,
      date: now.toISOString(),
    },
    {
      corridor_key: "USD:ISSUERA->EUR:ISSUERB",
      asset_a_code: "USD",
      asset_a_issuer: "ISSUERA",
      asset_b_code: "EUR",
      asset_b_issuer: "ISSUERB",
      success_rate: 99.1,
      total_transactions: 1890,
      successful_transactions: 1872,
      failed_transactions: 18,
      volume_usd: 720000,
      avg_settlement_latency_ms: 1850,
      liquidity_depth_usd: 4200000,
      date: now.toISOString(),
    },
    {
      corridor_key: "USDC:ISSUERC->SGD:ISSUERD",
      asset_a_code: "USDC",
      asset_a_issuer: "ISSUERC",
      asset_b_code: "SGD",
      asset_b_issuer: "ISSUERD",
      success_rate: 96.8,
      total_transactions: 1240,
      successful_transactions: 1201,
      failed_transactions: 39,
      volume_usd: 450000,
      avg_settlement_latency_ms: 3100,
      liquidity_depth_usd: 2800000,
      date: now.toISOString(),
    },
    {
      corridor_key: "EUR:ISSUERE->GBP:ISSUERF",
      asset_a_code: "EUR",
      asset_a_issuer: "ISSUERE",
      asset_b_code: "GBP",
      asset_b_issuer: "ISSUERF",
      success_rate: 97.8,
      total_transactions: 1560,
      successful_transactions: 1527,
      failed_transactions: 33,
      volume_usd: 580000,
      avg_settlement_latency_ms: 2100,
      liquidity_depth_usd: 3500000,
      date: now.toISOString(),
    },
    {
      corridor_key: "USDC:ISSUERG->JPY:ISSUERH",
      asset_a_code: "USDC",
      asset_a_issuer: "ISSUERG",
      asset_b_code: "JPY",
      asset_b_issuer: "ISSUERH",
      success_rate: 98.2,
      total_transactions: 980,
      successful_transactions: 963,
      failed_transactions: 17,
      volume_usd: 360000,
      avg_settlement_latency_ms: 2600,
      liquidity_depth_usd: 2100000,
      date: now.toISOString(),
    },
  ];

  // Generate historical data for liquidity
  const liquidityHistory: LiquidityDataPoint[] = lastSevenDays.flatMap((date) =>
    corridors.slice(0, 3).map((corridor) => ({
      timestamp: date.toISOString(),
      liquidity_usd: corridor.liquidity_depth_usd * (0.8 + Math.random() * 0.4),
      corridor_key: corridor.corridor_key,
    })),
  );

  // Generate TVL history
  const tvlHistory: TVLDataPoint[] = lastSevenDays.map((date) => ({
    timestamp: date.toISOString(),
    tvl_usd:
      16700000 +
      Math.random() * 2000000 -
      1000000 +
      (date.getTime() - lastSevenDays[0].getTime()) * 50000,
  }));

  // Generate settlement latency history
  const settlementLatencyHistory: SettlementLatencyDataPoint[] =
    lastSevenDays.map((date) => ({
      timestamp: date.toISOString(),
      median_latency_ms: 2300 + Math.random() * 600 - 300,
      p95_latency_ms: 4200 + Math.random() * 900 - 450,
      p99_latency_ms: 5800 + Math.random() * 1200 - 600,
    }));

  const totalVolume = corridors.reduce((sum, c) => sum + c.volume_usd, 0);
  const avgSuccessRate =
    corridors.reduce((sum, c) => sum + c.success_rate, 0) / corridors.length;

  return {
    top_corridors: corridors,
    liquidity_history: liquidityHistory,
    tvl_history: tvlHistory,
    settlement_latency_history: settlementLatencyHistory,
    total_volume_usd: totalVolume,
    avg_success_rate: avgSuccessRate,
    active_corridors: corridors.length,
  };
}
