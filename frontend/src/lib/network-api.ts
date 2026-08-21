/**
 * Network dashboard API client.
 * Backed by Stellar-Insightss/backend#15–#19:
 *   GET /api/v1/network/transactions-per-day → {date, count} (backend#16)
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

export interface NetworkTransactionsPerDayPoint {
  date: string;
  /** Total transaction count for the day. */
  count: number;
}

export interface NetworkTransactionsPerDayResponse {
  points: NetworkTransactionsPerDayPoint[];
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

function normalizeTransactionsPerDayPoints(
  raw: unknown,
): NetworkTransactionsPerDayPoint[] {
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
        transactions?: number;
        tx_count?: number;
      };
      const date = item.date ?? item.day ?? item.timestamp;
      const count = item.count ?? item.transactions ?? item.tx_count;
      if (!date || typeof count !== "number" || Number.isNaN(count))
        return null;
      return { date, count };
    })
    .filter((point): point is NetworkTransactionsPerDayPoint => point != null)
    .sort((a, b) => new Date(a.date).getTime() - new Date(b.date).getTime());
}

/**
 * Daily transaction count time series (backend#16).
 * Returns an empty series when the backend is unavailable so the panel
 * can show an empty state without failing the whole /network page.
 */
export async function fetchNetworkTransactionsPerDay(
  days = 30,
): Promise<NetworkTransactionsPerDayResponse> {
  const url = `${API_BASE}/api/v1/network/transactions-per-day?days=${days}`;
  try {
    const data = await fetchJson<unknown>(url);
    return { points: normalizeTransactionsPerDayPoints(data) };
  } catch (error) {
    const isNetworkError =
      error instanceof TypeError &&
      (error.message.includes("Failed to fetch") ||
        error.message.includes("Network request failed"));
    if (!isNetworkError) {
      logger.error("Failed to fetch network transactions-per-day:", error);
    }
    return { points: [] };
  }
}
