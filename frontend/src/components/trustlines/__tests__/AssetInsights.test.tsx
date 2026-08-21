import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { AssetInsights } from "../AssetInsights";
import type { AssetInsights as AssetInsightsType } from "@/lib/trustline-api";

describe("AssetInsights", () => {
  const baseData: AssetInsightsType = {
    asset_code: "USDC",
    asset_issuer: "GABC...XYZ",
    largest_holder_pct: 65,
    holder_growth_30d: 12,
    transfer_volume_30d: 500000,
    avg_transfer_volume: 400000,
    total_trustlines: 200,
  };

  it("delegates the loading state to the shared InsightsList", () => {
    render(<AssetInsights data={null} loading />);

    expect(screen.getByTestId("insights-loading")).toBeInTheDocument();
    expect(screen.queryByTestId("insights-list")).not.toBeInTheDocument();
  });

  it("shows an asset-specific empty message when there is no data", () => {
    render(<AssetInsights data={null} loading={false} />);

    expect(screen.getByTestId("insights-empty")).toHaveTextContent(
      "Insufficient data to generate insights for this asset.",
    );
  });

  it("shows the empty message when data yields no sentences", () => {
    const emptyData: AssetInsightsType = {
      asset_code: "USDC",
      asset_issuer: "GABC...XYZ",
      largest_holder_pct: null,
      holder_growth_30d: null,
      transfer_volume_30d: null,
      avg_transfer_volume: null,
      total_trustlines: 0,
    };

    render(<AssetInsights data={emptyData} loading={false} />);

    expect(screen.getByTestId("insights-empty")).toBeInTheDocument();
  });

  it("renders generated insight sentences on the happy path", () => {
    render(<AssetInsights data={baseData} loading={false} />);

    const list = screen.getByTestId("insights-list");
    expect(list).toBeInTheDocument();
    expect(screen.getByText(/largest single holder controls 65.0%/i)).toBeInTheDocument();
    expect(screen.getByText(/holder count grew by 12/i)).toBeInTheDocument();
  });
});
