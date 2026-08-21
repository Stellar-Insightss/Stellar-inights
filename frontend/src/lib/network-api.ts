/**
 * Network dashboard API client.
 * Backed by Stellar-Insightss/backend#15–#19:
 *   GET /api/v1/network/new-accounts    → {date, count} (backend#18)
 *   GET /api/v1/network/payment-volume  → {date, volume} (backend#17)
 *
 * Volume unit matches NetworkStats.volume_24h (USD-equivalent) —
 * today's series point should align with that single-stat metric.
 */
import { logger } from "@/lib/logger";
import { config } from "@/config";

const API_BASE = config.apiUrl;

export interface NetworkPaymentVolumePoint {
  date: string;
  /** USD-equivalent payment volume for the day (same convention as volume_24h). */
  volume: number;
}

export interface NetworkPaymentVolumeResponse {
  points: NetworkPaymentVolumePoint[];
  unit: "usd";
}

export interface NetworkNewAccountsPoint {
  date: string;
  /** New accounts created that day. */
  count: number;
}

export interface NetworkNewAccountsResponse {
  points: NetworkNewAccountsPoint[];
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

function normalizePaymentVolumePoints(raw: unknown): NetworkPaymentVolumePoint[] {
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
        volume?: number;
        volume_usd?: number;
        payment_volume?: number;
      };
      const date = item.date ?? item.day ?? item.timestamp;
      const volume = item.volume ?? item.volume_usd ?? item.payment_volume;
      if (!date || typeof volume !== "number" || Number.isNaN(volume)) return null;
      return { date, volume };
    })
    .filter((point): point is NetworkPaymentVolumePoint => point != null)
    .sort((a, b) => new Date(a.date).getTime() - new Date(b.date).getTime());
}

/**
 * Daily payment volume time series.
 * Returns an empty series when the backend is unavailable so the panel
 * can show an empty state without failing the whole /network page.
 */
export async function fetchNetworkPaymentVolume(
  days = 30,
): Promise<NetworkPaymentVolumeResponse> {
  const url = `${API_BASE}/api/v1/network/payment-volume?days=${days}`;
  try {
    const data = await fetchJson<unknown>(url);
    return {
      points: normalizePaymentVolumePoints(data),
      unit: "usd",
    };
  } catch (error) {
    const isNetworkError =
      error instanceof TypeError &&
      (error.message.includes("Failed to fetch") ||
        error.message.includes("Network request failed"));
    if (!isNetworkError) {
      logger.error("Failed to fetch network payment volume:", error);
    }
    return { points: [], unit: "usd" };
  }
}

function normalizeNewAccountsPoints(raw: unknown): NetworkNewAccountsPoint[] {
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
        new_accounts?: number;
        created_accounts?: number;
      };
      const date = item.date ?? item.day ?? item.timestamp;
      const count = item.count ?? item.new_accounts ?? item.created_accounts;
      if (!date || typeof count !== "number" || Number.isNaN(count))
        return null;
      return { date, count };
    })
    .filter((point): point is NetworkNewAccountsPoint => point != null)
    .sort((a, b) => new Date(a.date).getTime() - new Date(b.date).getTime());
}

/**
 * New-accounts-per-day time series (backend#18).
 * Returns an empty series when the backend is unavailable so the panel
 * can show an empty state without failing the whole /network page.
 */
export async function fetchNetworkNewAccounts(
  days = 30,
): Promise<NetworkNewAccountsResponse> {
  const url = `${API_BASE}/api/v1/network/new-accounts?days=${days}`;
  try {
    const data = await fetchJson<unknown>(url);
    return { points: normalizeNewAccountsPoints(data) };
  } catch (error) {
    const isNetworkError =
      error instanceof TypeError &&
      (error.message.includes("Failed to fetch") ||
        error.message.includes("Network request failed"));
    if (!isNetworkError) {
      logger.error("Failed to fetch network new accounts:", error);
    }
    return { points: [] };
  }
}
