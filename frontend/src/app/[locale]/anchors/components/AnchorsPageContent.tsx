"use client";
import { useEffect, useState } from "react";
import {
  Search,
  Home as AnchorIcon,
} from "lucide-react";
import { MainLayout } from "@/components/layout";
import { DataTablePagination } from "@/components/ui/DataTablePagination";
import { ExportDialog } from "@/components/ExportDialog";
import {
  formatNumber,
  Error,
  SearchAndControls
} from "./helpers";
import { SkeletonTable } from "@/components/ui/Skeleton";
import useAnchorPage from "./useAnchorPage";
import AnchorList from "./AnchorTable";
import AnchorCards from "./AnchorCards";
import { InsightsList } from "@/components/dashboard/InsightsList";
import { getAnchorInsights } from "@/lib/api/anchor";
import { logger } from "@/lib/logger";


const AnchorsPageContent = () => {
  const {
    anchors,
    loading,
    error,
    searchTerm,
    setSearchTerm,
    sortBy,
    setSortBy,
    sortOrder,
    setSortOrder,
    isExportOpen,
    setIsExportOpen,
    currentPage,
    pageSize,
    onPageChange,
    onPageSizeChange,
    paginatedAnchors, sortedAndFilteredAnchors
  } = useAnchorPage()

  const [insights, setInsights] = useState<string[]>([]);
  const [insightsLoading, setInsightsLoading] = useState(true);
  const [insightsError, setInsightsError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    async function fetchInsights() {
      setInsightsLoading(true);
      setInsightsError(null);
      try {
        const result = await getAnchorInsights();
        if (!cancelled) setInsights(result.insights ?? []);
      } catch (err) {
        logger.error("Error fetching anchor insights:", err);
        if (!cancelled) {
          setInsightsError("Unable to load anchor insights right now.");
        }
      } finally {
        if (!cancelled) setInsightsLoading(false);
      }
    }
    fetchInsights();
    return () => {
      cancelled = true;
    };
  }, []);


  return (
    <MainLayout>
      <div className="p-4 sm:p-6 lg:p-8 max-w-7xl mx-auto">
        {/* Page Header */}
        <div className="mb-8">
          <h1 className="text-3xl font-bold text-gray-900 dark:text-white mb-2 flex items-center gap-2">
            <AnchorIcon className="w-8 h-8 text-blue-500" />
            Anchor Analytics
          </h1>
          <p className="text-gray-600 dark:text-gray-400">
            Monitor anchor reliability, asset coverage, and transaction success
            rates
          </p>
        </div>

        {/* Error Message */}
        {error && (
          <Error error={error} />
        )}

        <section
          aria-labelledby="anchor-insights-heading"
          className="mb-6 bg-white dark:bg-slate-800 rounded-lg border border-gray-200 dark:border-slate-700 p-4"
        >
          <h2
            id="anchor-insights-heading"
            className="text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wide mb-3"
          >
            Insights
          </h2>
          <InsightsList
            insights={insights}
            isLoading={insightsLoading}
            error={insightsError}
            emptyMessage="No anchor insights available yet."
          />
        </section>

        <SearchAndControls
          searchTerm={searchTerm} setSearchTerm={setSearchTerm} sortBy={sortBy} setSortBy={setSortBy} setSortOrder={setSortOrder} sortOrder={sortOrder} setIsExportOpen={setIsExportOpen}
        />

        {!loading && !error && sortedAndFilteredAnchors.length > 0 && (
          <p className="text-xs text-gray-500 dark:text-gray-400 mt-2 mb-4">
            💡 Click on any row to view anchor details • Click column headers to
            sort
          </p>
        )}
        <div className="space-y-4">
          <div className="bg-white dark:bg-slate-800 rounded-lg border border-gray-200 dark:border-slate-700 overflow-hidden">
            {loading ? (
              <SkeletonTable rows={8} />
            ) : (
              <>
                <AnchorList sortBy={sortBy}
                  sortOrder={sortOrder}
                  setSortBy={setSortBy}
                  setSortOrder={setSortOrder}
                  paginatedAnchors={paginatedAnchors} />
                <AnchorCards paginatedAnchors={paginatedAnchors} />
              </>
            )}
          </div>

          {/* Pagination */}
          {!loading && !error && sortedAndFilteredAnchors.length > 0 && (
            <DataTablePagination
              totalItems={sortedAndFilteredAnchors.length}
              pageSize={pageSize}
              currentPage={currentPage}
              onPageChange={onPageChange}
              onPageSizeChange={onPageSizeChange}
            />
          )}
        </div>

        {/* Summary Stats */}
        {!loading && !error && sortedAndFilteredAnchors.length > 0 && (
          <div className="mt-8 grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
            <div className="bg-white dark:bg-slate-800 rounded-lg border border-gray-200 dark:border-slate-700 p-4">
              <div className="text-sm text-muted-foreground dark:text-muted-foreground mb-1">
                Total Anchors
              </div>
              <div className="text-2xl font-bold text-gray-900 dark:text-white">
                {sortedAndFilteredAnchors.length}
              </div>
            </div>
            <div className="bg-white dark:bg-slate-800 rounded-lg border border-gray-200 dark:border-slate-700 p-4">
              <div className="text-sm text-muted-foreground dark:text-muted-foreground mb-1">
                Avg Reliability
              </div>
              <div className="text-2xl font-bold text-gray-900 dark:text-white">
                {sortedAndFilteredAnchors.length > 0
                  ? (
                    sortedAndFilteredAnchors.reduce(
                      (sum, a) => sum + a.reliability_score,
                      0,
                    ) / sortedAndFilteredAnchors.length
                  ).toFixed(1)
                  : "0.0"}
                %
              </div>
            </div>
            <div className="bg-white dark:bg-slate-800 rounded-lg border border-gray-200 dark:border-slate-700 p-4">
              <div className="text-sm text-muted-foreground dark:text-muted-foreground mb-1">
                Total Transactions
              </div>
              <div className="text-2xl font-bold text-gray-900 dark:text-white">
                {formatNumber(
                  sortedAndFilteredAnchors.reduce(
                    (sum, a) => sum + a.total_transactions,
                    0,
                  ),
                )}
              </div>
            </div>
            <div className="bg-white dark:bg-slate-800 rounded-lg border border-gray-200 dark:border-slate-700 p-4">
              <div className="text-sm text-muted-foreground dark:text-muted-foreground mb-1">
                Healthy Anchors
              </div>
              <div className="text-2xl font-bold text-green-600 dark:text-green-400">
                {
                  sortedAndFilteredAnchors.filter(
                    (a) =>
                      a.status.toLowerCase() === "green" ||
                      a.status === "Healthy",
                  ).length
                }
              </div>
            </div>
          </div>
        )}

        {/* Empty State (when no error but also no data) */}
        {!loading &&
          !error &&
          sortedAndFilteredAnchors.length === 0 &&
          anchors.length > 0 && (
            <div className="bg-white dark:bg-slate-800 rounded-lg border border-gray-200 dark:border-slate-700 p-12 text-center">
              <Search className="w-12 h-12 text-gray-400 mx-auto mb-4" />
              <p className="text-gray-600 dark:text-gray-400">
                No anchors found matching &quot;{searchTerm}&quot;
              </p>
            </div>
          )}
      </div>
      <ExportDialog
        isOpen={isExportOpen}
        onClose={() => setIsExportOpen(false)}
        type="anchors"
        title="Stellar Anchors"
      />
    </MainLayout>
  );
};

export default AnchorsPageContent;