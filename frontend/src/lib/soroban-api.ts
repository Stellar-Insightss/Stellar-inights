/**
 * Soroban analytics API client.
 * Backed by backend endpoints from Stellar-Insightss/backend#21–#24:
 *   GET /api/v1/soroban/contract-calls
 *   GET /api/v1/soroban/top-contracts
 *   GET /api/v1/soroban/new-deployments
 *   GET /api/v1/soroban/gas-usage           (backend#23)
 *   GET /api/v1/soroban/active-contracts    (backend#21)
 *
 * New deployments may be partial until contracts-repo deployment/init
 * events land (backend#24). The API surfaces that via `partial: true`.
 *
 * Contract-calls counts are daily event counts from `contract_events`
 * (backend#22) — one transaction may emit multiple events.
 */
import { logger } from "@/lib/logger";
import { config } from "@/config";

const API_BASE = config.apiUrl;

export interface SorobanTopContract {
  contract_id: string;
  call_count: number;
  event_count?: number;
  last_seen_at?: string;
}

export interface SorobanTopContractsResponse {
  window: string;
  contracts: SorobanTopContract[];
}

export interface SorobanDeployment {
  contract_id: string;
  deployed_at: string;
  deployer?: string | null;
  ledger?: number | null;
  wasm_hash?: string | null;
}

export interface SorobanNewDeploymentsResponse {
  /** True when deployment/init event coverage is incomplete. */
  partial: boolean;
  deployments: SorobanDeployment[];
  /** Human-readable note when data is partial or empty due to upstream gaps. */
  notice?: string | null;
}

export interface SorobanContractCallPoint {
  date: string;
  count: number;
}

export interface SorobanContractCallsResponse {
  points: SorobanContractCallPoint[];
  /** Documented definition: event rows, not distinct transactions. */
  metric: "events";
}

export interface SorobanGasUsageResponse {
  /** Total gas consumed across all contracts. */
  total_gas: number;
  /** Average gas per transaction/operation. */
  avg_gas?: number;
  /** Optional time window label. */
  window?: string;
  /** Optional trend percentage. */
  trend?: number;
  /** Set to true when backend#23 hasn't landed yet. */
  coming_soon?: boolean;
}

export interface SorobanActiveContractsResponse {
  /** Total count of active contracts. */
  count: number;
  /** Optional time window label (e.g., "7d", "30d"). */
  window?: string;
  /** Optional trend percentage vs previous period. */
  trend?: number;
}

async function fetchJson<T>(url: string): Promise<T> {
  const response = await fetch(url, {
    method: "GET",
    headers: { "Content-Type": "application/json" },
  });
  if (!response.ok) {
    throw new Error(`API error: ${response.status}`);
  }
  return response.json();
}

function normalizeContractCallPoints(raw: unknown): SorobanContractCallPoint[] {
  const rows = Array.isArray(raw)
    ? raw
    : Array.isArray((raw as { points?: unknown })?.points)
      ? (raw as { points: unknown[] }).points
      : Array.isArray((raw as { series?: unknown })?.series)
        ? (raw as { series: unknown[] }).series
        : Array.isArray((raw as { data?: unknown })?.data)
          ? (raw as { data: unknown[] }).data
          : [];

  return rows
    .map((row) => {
      const item = row as {
        date?: string;
        day?: string;
        timestamp?: string;
        count?: number;
        call_count?: number;
      };
      const date = item.date ?? item.day ?? item.timestamp;
      const count = item.count ?? item.call_count;
      if (!date || typeof count !== "number" || Number.isNaN(count)) return null;
      return { date, count };
    })
    .filter((point): point is SorobanContractCallPoint => point != null)
    .sort((a, b) => new Date(a.date).getTime() - new Date(b.date).getTime());
}

/**
 * Daily contract-call (event) counts over a window.
 * Returns an empty series when the backend is unavailable.
 */
export async function fetchSorobanContractCalls(
  days = 30,
): Promise<SorobanContractCallsResponse> {
  const url = `${API_BASE}/api/v1/soroban/contract-calls?days=${days}`;
  try {
    const data = await fetchJson<unknown>(url);
    return {
      points: normalizeContractCallPoints(data),
      metric: "events",
    };
  } catch (error) {
    const isNetworkError =
      error instanceof TypeError &&
      (error.message.includes("Failed to fetch") ||
        error.message.includes("Network request failed"));
    if (!isNetworkError) {
      logger.error("Failed to fetch Soroban contract calls:", error);
    }
    return { points: [], metric: "events" };
  }
}

/**
 * Top contracts by call/event count over a window.
 * Returns an empty list (not mock data) when the backend is unavailable
 * so the dashboard can show a real empty state.
 */
export async function fetchSorobanTopContracts(
  limit = 20,
  window = "7d",
): Promise<SorobanTopContractsResponse> {
  const url = `${API_BASE}/api/v1/soroban/top-contracts?limit=${limit}&window=${encodeURIComponent(window)}`;
  try {
    const data = await fetchJson<SorobanTopContractsResponse>(url);
    return {
      window: data.window ?? window,
      contracts: Array.isArray(data.contracts) ? data.contracts : [],
    };
  } catch (error) {
    const isNetworkError =
      error instanceof TypeError &&
      (error.message.includes("Failed to fetch") ||
        error.message.includes("Network request failed"));
    if (!isNetworkError) {
      logger.error("Failed to fetch Soroban top contracts:", error);
    }
    return { window, contracts: [] };
  }
}

/**
 * Recent contract deployments.
 * Always preserves the `partial` signal — never fabricates a complete list
 * when the backend is down or returns incomplete coverage.
 */
export async function fetchSorobanNewDeployments(
  limit = 20,
): Promise<SorobanNewDeploymentsResponse> {
  const url = `${API_BASE}/api/v1/soroban/new-deployments?limit=${limit}`;
  try {
    const data = await fetchJson<SorobanNewDeploymentsResponse>(url);
    return {
      partial: Boolean(data.partial),
      deployments: Array.isArray(data.deployments) ? data.deployments : [],
      notice: data.notice ?? null,
    };
  } catch (error) {
    const isNetworkError =
      error instanceof TypeError &&
      (error.message.includes("Failed to fetch") ||
        error.message.includes("Network request failed"));
    if (!isNetworkError) {
      logger.error("Failed to fetch Soroban new deployments:", error);
    }
    return {
      partial: true,
      deployments: [],
      notice:
        "New deployments data is unavailable or incomplete until contract deployment/init events are fully ingested.",
    };
  }
}

/**
 * Gas usage statistics (backend#23).
 * Returns coming_soon: true when the backend endpoint hasn't landed yet
 * (404 / 501 response), so the dashboard can show an explicit placeholder
 * rather than an error state.
 */
export async function fetchSorobanGasUsage(
  window = "7d",
): Promise<SorobanGasUsageResponse> {
  const url = `${API_BASE}/api/v1/soroban/gas-usage?window=${encodeURIComponent(window)}`;
  try {
    const data = await fetchJson<SorobanGasUsageResponse>(url);
    return {
      total_gas: typeof data.total_gas === "number" ? data.total_gas : 0,
      avg_gas: typeof data.avg_gas === "number" ? data.avg_gas : undefined,
      window: data.window ?? window,
      trend: typeof data.trend === "number" ? data.trend : undefined,
      coming_soon: Boolean(data.coming_soon),
    };
  } catch (error) {
    // 404 / 501 means backend#23 hasn't landed yet — surface "coming soon"
    const isNotImplemented =
      error instanceof Error &&
      (error.message.includes("404") || error.message.includes("501"));
    const isNetworkError =
      error instanceof TypeError &&
      (error.message.includes("Failed to fetch") ||
        error.message.includes("Network request failed"));

    if (!isNetworkError && !isNotImplemented) {
      logger.error("Failed to fetch Soroban gas usage:", error);
    }

    return {
      total_gas: 0,
      window,
      coming_soon: true,
    };
  }
}

/**
 * Active contracts count (backend#21).
 * Returns zero count when backend is unavailable.
 */
export async function fetchSorobanActiveContracts(
  window = "7d",
): Promise<SorobanActiveContractsResponse> {
  const url = `${API_BASE}/api/v1/soroban/active-contracts?window=${encodeURIComponent(window)}`;
  try {
    const data = await fetchJson<SorobanActiveContractsResponse>(url);
    return {
      count: typeof data.count === "number" ? data.count : 0,
      window: data.window ?? window,
      trend: typeof data.trend === "number" ? data.trend : undefined,
    };
  } catch (error) {
    const isNetworkError =
      error instanceof TypeError &&
      (error.message.includes("Failed to fetch") ||
        error.message.includes("Network request failed"));

    if (!isNetworkError) {
      logger.error("Failed to fetch Soroban active contracts:", error);
    }
    return { count: 0, window };
  }
}
