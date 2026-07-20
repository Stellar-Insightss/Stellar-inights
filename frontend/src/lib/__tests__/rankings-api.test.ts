/**
 * Asset Rankings API Client Tests
 */
import { describe, it, expect, beforeEach, vi } from "vitest";

vi.mock("@/config", () => ({
  config: { apiUrl: "http://localhost:8080" },
}));

vi.mock("@/lib/logger", () => ({
  logger: { error: vi.fn(), warn: vi.fn(), debug: vi.fn(), info: vi.fn() },
}));

import { fetchAssetRankings } from "../rankings-api";

global.fetch = vi.fn();
const mockFetch = vi.mocked(global.fetch);

describe("fetchAssetRankings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("requests the backend#39 endpoint with the requested sort mode and limit", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: async () => [],
    } as Response);

    await fetchAssetRankings("volume", 25);

    expect(global.fetch).toHaveBeenCalledWith(
      "http://localhost:8080/api/v1/rankings/assets?sort_by=volume&limit=25",
      expect.objectContaining({ method: "GET" }),
    );
  });

  it("defaults to sorting by holders with a limit of 50", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: async () => [],
    } as Response);

    await fetchAssetRankings();

    expect(global.fetch).toHaveBeenCalledWith(
      "http://localhost:8080/api/v1/rankings/assets?sort_by=holders&limit=50",
      expect.anything(),
    );
  });

  it("returns the API response verbatim on success", async () => {
    const apiResponse = [
      {
        rank: 1,
        asset_code: "XLM",
        asset_issuer: "native",
        holder_count: 1000,
        volume_24h_usd: 5000,
      },
    ];
    mockFetch.mockResolvedValueOnce({
      ok: true,
      json: async () => apiResponse,
    } as Response);

    const result = await fetchAssetRankings("holders");

    expect(result).toEqual(apiResponse);
  });

  it("falls back to mock rankings sorted by holder count when the request fails", async () => {
    mockFetch.mockResolvedValueOnce({
      ok: false,
      status: 503,
    } as Response);

    const result = await fetchAssetRankings("holders");

    expect(result.length).toBeGreaterThan(0);
    expect(result).toEqual([...result].sort((a, b) => b.holder_count - a.holder_count));
    result.forEach((asset, index) => expect(asset.rank).toBe(index + 1));
  });

  it("falls back to mock rankings sorted by volume when the network is unreachable", async () => {
    mockFetch.mockRejectedValueOnce(new TypeError("Failed to fetch"));

    const result = await fetchAssetRankings("volume");

    expect(result.length).toBeGreaterThan(0);
    expect(result).toEqual(
      [...result].sort((a, b) => b.volume_24h_usd - a.volume_24h_usd),
    );
    result.forEach((asset, index) => expect(asset.rank).toBe(index + 1));
  });
});
