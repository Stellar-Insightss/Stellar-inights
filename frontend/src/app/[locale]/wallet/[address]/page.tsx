"use client";

import { useEffect, useState, use, Suspense, useCallback } from "react";
import { useRouter } from "@/i18n/navigation";
import { Link } from "@/i18n/navigation";
import { AlertCircle, Search, Wallet, ArrowRight, Copy, Check } from "lucide-react";
import { logger } from "@/lib/logger";
import { getAddressValidationError, isStellarAccountAddress, formatAddressShort } from "@/lib/address";
import { useWallet } from "@/components/lib/wallet-context";
import { BackButton } from "@/components/ui/BackButton";
import { Skeleton, SkeletonChart, SkeletonTable } from "@/components/ui/Skeleton";
import { BalanceHistoryChart } from "@/components/charts/BalanceHistoryChart";
import { PortfolioValuePanel } from "@/components/wallet/PortfolioValuePanel";
import { AssetAllocationChart } from "@/components/wallet/AssetAllocationChart";
import { ActivityCalendar } from "@/components/wallet/ActivityCalendar";
import { LargestTransfersTable } from "@/components/wallet/LargestTransfersTable";
import {
  fetchWalletBalanceHistory,
  fetchWalletPortfolio,
  fetchWalletAllocation,
  fetchWalletActivity,
  fetchWalletTransfers,
  WalletPortfolioSnapshot,
  WalletAssetAllocation,
  WalletActivityDay,
  WalletTransfer,
  BalanceHistoryDataPoint,
} from "@/lib/analytics-api";

// ─── Types ────────────────────────────────────────────────────────────────────

interface WalletData {
  portfolio: WalletPortfolioSnapshot;
  balanceHistory: BalanceHistoryDataPoint[];
  allocations: WalletAssetAllocation[];
  activity: WalletActivityDay[];
  transfers: WalletTransfer[];
}

// ─── Loading skeleton ─────────────────────────────────────────────────────────

function WalletPageSkeleton() {
  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8 space-y-6">
      {/* Breadcrumb */}
      <div className="flex items-center gap-2">
        <Skeleton className="h-4 w-20 rounded" />
        <Skeleton className="h-4 w-4 rounded" />
        <Skeleton className="h-4 w-48 rounded" />
      </div>
      {/* Top row */}
      <div className="grid gap-6 lg:grid-cols-12">
        <div className="lg:col-span-8">
          <Skeleton className="h-64 w-full rounded-2xl" />
        </div>
        <div className="lg:col-span-4">
          <Skeleton className="h-64 w-full rounded-2xl" />
        </div>
      </div>
      {/* Balance history */}
      <SkeletonChart height={350} className="rounded-2xl" />
      {/* Bottom row */}
      <div className="grid gap-6 lg:grid-cols-12">
        <div className="lg:col-span-8">
          <Skeleton className="h-56 w-full rounded-2xl" />
        </div>
        <div className="lg:col-span-4">
          <Skeleton className="h-56 w-full rounded-2xl" />
        </div>
      </div>
      {/* Transfers */}
      <SkeletonTable rows={6} />
    </div>
  );
}

// ─── Invalid address state ────────────────────────────────────────────────────

function InvalidAddressState({ address, message }: { address: string; message: string }) {
  const [searchInput, setSearchInput] = useState("");
  const router = useRouter();

  function handleSearch(e: React.FormEvent) {
    e.preventDefault();
    const trimmed = searchInput.trim();
    if (isStellarAccountAddress(trimmed)) {
      router.push(`/wallet/${trimmed}`);
    }
  }

  return (
    <div className="max-w-xl mx-auto px-4 py-24 flex flex-col items-center text-center">
      <div className="w-16 h-16 rounded-full bg-amber-500/10 border border-amber-500/20 flex items-center justify-center mb-6">
        <AlertCircle className="w-8 h-8 text-amber-400" aria-hidden="true" />
      </div>
      <div className="text-[10px] font-mono text-accent uppercase tracking-[0.2em] mb-3">
        Address Error
      </div>
      <h1 className="text-2xl font-black tracking-tighter uppercase italic mb-3">
        Invalid Stellar Address
      </h1>
      <p className="text-sm text-muted-foreground font-mono mb-2 break-all">
        <span className="text-foreground/50">{address.length > 24 ? `${address.slice(0, 12)}…${address.slice(-12)}` : address}</span>
      </p>
      <p className="text-xs text-muted-foreground mb-8">{message}</p>

      {/* Quick search for a valid address */}
      <form onSubmit={handleSearch} className="w-full flex gap-2">
        <input
          type="text"
          value={searchInput}
          onChange={(e) => setSearchInput(e.target.value)}
          placeholder="Enter a valid G… or M… address"
          className="flex-1 bg-slate-900/50 border border-border/60 rounded-xl px-4 py-2.5 text-sm font-mono text-foreground placeholder:text-muted-foreground/40 focus:outline-none focus:border-accent/50 transition-colors"
          aria-label="Stellar address to look up"
          spellCheck={false}
          autoComplete="off"
        />
        <button
          type="submit"
          disabled={!isStellarAccountAddress(searchInput.trim())}
          className="px-4 py-2.5 bg-accent hover:bg-accent/80 disabled:bg-slate-800 disabled:text-muted-foreground text-white rounded-xl text-sm font-bold transition-colors flex items-center gap-2"
          aria-label="Look up address"
        >
          <Search className="w-4 h-4" aria-hidden="true" />
          Go
        </button>
      </form>
    </div>
  );
}

// ─── Empty state (valid address, no data) ─────────────────────────────────────

function EmptyWalletState({ address }: { address: string }) {
  return (
    <div className="max-w-xl mx-auto px-4 py-24 flex flex-col items-center text-center">
      <div className="w-16 h-16 rounded-full bg-accent/10 border border-accent/20 flex items-center justify-center mb-6">
        <Wallet className="w-8 h-8 text-accent" aria-hidden="true" />
      </div>
      <div className="text-[10px] font-mono text-accent uppercase tracking-[0.2em] mb-3">
        No Activity Found
      </div>
      <h1 className="text-2xl font-black tracking-tighter uppercase italic mb-3">
        Empty Wallet
      </h1>
      <p className="text-sm text-muted-foreground font-mono mb-1 break-all">
        {formatAddressShort(address)}
      </p>
      <p className="text-xs text-muted-foreground/60 mt-2 max-w-sm">
        This address exists on the Stellar network but has no recorded transactions yet. Data will appear once activity is detected.
      </p>
    </div>
  );
}

// ─── Error state ──────────────────────────────────────────────────────────────

function ErrorState({ message }: { message: string }) {
  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-16 flex flex-col items-center justify-center text-center">
      <div className="w-16 h-16 bg-rose-500/10 rounded-full flex items-center justify-center mb-4">
        <AlertCircle className="w-8 h-8 text-rose-500" aria-hidden="true" />
      </div>
      <h2 className="text-xl font-bold text-white mb-2">Error Loading Wallet</h2>
      <p className="text-slate-400 mb-6 max-w-md text-sm font-mono">{message}</p>
      <Link
        href="/"
        className="px-4 py-2 bg-slate-800 hover:bg-slate-700 text-white rounded-lg transition-colors font-medium text-sm"
      >
        Return to Dashboard
      </Link>
    </div>
  );
}

// ─── Address header bar ───────────────────────────────────────────────────────

function AddressHeader({ address }: { address: string }) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(address);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // clipboard not available
    }
  }

  return (
    <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 border-b border-border/50 pb-5">
      <div>
        <div className="text-[10px] font-mono text-accent uppercase tracking-[0.2em] mb-1">
          Wallet Dashboard
        </div>
        <h1 className="text-2xl font-black tracking-tighter uppercase italic flex items-center gap-3">
          <Wallet className="w-6 h-6 text-accent" aria-hidden="true" />
          {formatAddressShort(address, 10, 10)}
        </h1>
      </div>
      <button
        onClick={handleCopy}
        className="self-start sm:self-auto flex items-center gap-2 px-3 py-2 rounded-xl bg-slate-900/40 border border-white/5 hover:border-accent/20 transition-colors text-xs font-mono text-muted-foreground hover:text-foreground"
        aria-label={copied ? "Address copied" : "Copy full address"}
      >
        {copied ? (
          <>
            <Check className="w-3.5 h-3.5 text-emerald-400" aria-hidden="true" />
            <span className="text-emerald-400">Copied</span>
          </>
        ) : (
          <>
            <Copy className="w-3.5 h-3.5" aria-hidden="true" />
            Copy address
          </>
        )}
      </button>
    </div>
  );
}

// ─── My Wallet shortcut banner ────────────────────────────────────────────────

function MyWalletBanner({ connectedAddress, currentAddress }: { connectedAddress: string; currentAddress: string }) {
  const router = useRouter();

  if (connectedAddress === currentAddress) return null;

  return (
    <div className="flex items-center justify-between gap-4 px-4 py-3 rounded-xl bg-accent/5 border border-accent/15 text-xs font-mono">
      <div className="flex items-center gap-2 min-w-0">
        <div className="w-2 h-2 rounded-full bg-accent shrink-0" aria-hidden="true" />
        <span className="text-muted-foreground truncate">
          Connected wallet: <span className="text-foreground">{formatAddressShort(connectedAddress, 8, 8)}</span>
        </span>
      </div>
      <button
        onClick={() => router.push(`/wallet/${connectedAddress}`)}
        className="flex items-center gap-1 text-accent hover:text-accent/80 font-bold uppercase tracking-wider transition-colors shrink-0"
        aria-label="View my connected wallet"
      >
        My Wallet
        <ArrowRight className="w-3 h-3" aria-hidden="true" />
      </button>
    </div>
  );
}

// ─── Main page content ────────────────────────────────────────────────────────

function WalletPageContent({ params }: { params: Promise<{ address: string }> }) {
  const { address } = use(params);
  const { isConnected, address: connectedAddress } = useWallet();

  const [walletData, setWalletData] = useState<WalletData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [validationError, setValidationError] = useState<string | null>(null);
  const [isEmpty, setIsEmpty] = useState(false);

  const loadData = useCallback(async () => {
    if (!address) return;

    // Validate address format first — before any network call
    const addrError = getAddressValidationError(address);
    if (addrError) {
      setValidationError(addrError);
      setLoading(false);
      return;
    }

    try {
      setLoading(true);
      setError(null);

      // Fetch all 5 data sources in parallel
      const [portfolio, balanceHistRes, allocationRes, activityRes, transfersRes] =
        await Promise.all([
          fetchWalletPortfolio(address),
          fetchWalletBalanceHistory(address),
          fetchWalletAllocation(address),
          fetchWalletActivity(address),
          fetchWalletTransfers(address),
        ]);

      // Treat a wallet with zero historical balance points and no transfers as "empty"
      const hasData =
        balanceHistRes.balance_history.length > 0 ||
        allocationRes.allocations.length > 0 ||
        transfersRes.transfers.length > 0;

      if (!hasData) {
        setIsEmpty(true);
        setLoading(false);
        return;
      }

      setWalletData({
        portfolio,
        balanceHistory: balanceHistRes.balance_history,
        allocations: allocationRes.allocations,
        activity: activityRes.activity,
        transfers: transfersRes.transfers,
      });
    } catch (err) {
      logger.error("Failed to load wallet data:", err instanceof Error ? err : new Error(String(err)));
      setError("Failed to load wallet data. Please try again later.");
    } finally {
      setLoading(false);
    }
  }, [address]);

  useEffect(() => {
    loadData();
  }, [loadData]);

  // ── Render states ──────────────────────────────────────────────────────────

  if (loading) return <WalletPageSkeleton />;

  if (validationError) {
    return <InvalidAddressState address={address} message={validationError} />;
  }

  if (error) return <ErrorState message={error} />;

  if (isEmpty) return <EmptyWalletState address={address} />;

  if (!walletData) return null;

  const { portfolio, balanceHistory, allocations, activity, transfers } = walletData;

  // ── Happy path ─────────────────────────────────────────────────────────────

  return (
    <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8 space-y-6 animate-in fade-in slide-in-from-bottom-4 duration-500">
      {/* Breadcrumb */}
      <div className="flex items-center gap-2 text-sm text-slate-400">
        <BackButton
          fallbackHref="/wallet"
          label="Wallet Insights"
          className="hover:text-white transition-colors flex items-center gap-1 group"
        />
        <span className="text-slate-600">/</span>
        <span className="text-slate-200 font-mono text-xs truncate max-w-[220px]">
          {formatAddressShort(address)}
        </span>
      </div>

      {/* My wallet shortcut banner (only if a different wallet is connected) */}
      {isConnected && connectedAddress && (
        <MyWalletBanner connectedAddress={connectedAddress} currentAddress={address} />
      )}

      {/* Page title + copy button */}
      <AddressHeader address={address} />

      {/* Row 1: Portfolio value (wide) + Asset allocation (narrow) */}
      <div className="grid gap-6 lg:grid-cols-12">
        <div className="lg:col-span-8">
          <PortfolioValuePanel data={portfolio} />
        </div>
        <div className="lg:col-span-4">
          <AssetAllocationChart allocations={allocations} address={address} />
        </div>
      </div>

      {/* Row 2: Balance history (full width) */}
      <BalanceHistoryChart data={balanceHistory} address={address} />

      {/* Row 3: Activity calendar (wide) + quick stats (narrow) */}
      <div className="grid gap-6 lg:grid-cols-12">
        <div className="lg:col-span-8">
          <ActivityCalendar activity={activity} address={address} />
        </div>
        <div className="lg:col-span-4 flex flex-col gap-4">
          {/* Mini stat cards to fill the right column beside the calendar */}
          <div className="glass-card rounded-2xl p-5 border border-border/50 flex-1 flex flex-col justify-center">
            <div className="text-[10px] font-mono text-accent uppercase tracking-[0.2em] mb-4">
              Quick Stats
            </div>
            <div className="space-y-4">
              <div className="flex justify-between items-baseline">
                <span className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider">
                  Avg txns / active day
                </span>
                <span className="font-black font-mono text-foreground tabular-nums">
                  {(() => {
                    const activeDays = activity.filter((d) => d.count > 0).length;
                    const total = activity.reduce((s, d) => s + d.count, 0);
                    return activeDays > 0 ? (total / activeDays).toFixed(1) : "—";
                  })()}
                </span>
              </div>
              <div className="flex justify-between items-baseline">
                <span className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider">
                  Busiest single day
                </span>
                <span className="font-black font-mono text-foreground tabular-nums">
                  {Math.max(...activity.map((d) => d.count), 0)}
                </span>
              </div>
              <div className="flex justify-between items-baseline">
                <span className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider">
                  Tokens held
                </span>
                <span className="font-black font-mono text-foreground tabular-nums">
                  {portfolio.total_assets}
                </span>
              </div>
              <div className="flex justify-between items-baseline">
                <span className="text-[10px] font-mono text-muted-foreground uppercase tracking-wider">
                  30d transactions
                </span>
                <span className="font-black font-mono text-foreground tabular-nums">
                  {portfolio.total_transactions_30d}
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Row 4: Largest transfers (full width) */}
      <LargestTransfersTable transfers={transfers} address={address} />
    </div>
  );
}

// ─── Export ───────────────────────────────────────────────────────────────────

export default function WalletPage(props: { params: Promise<{ address: string }> }) {
  return (
    <Suspense
      fallback={
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8 flex items-center justify-center min-h-[400px]">
          <div
            className="w-8 h-8 border-4 border-accent border-t-transparent rounded-full animate-spin"
            role="status"
            aria-label="Loading wallet"
          />
        </div>
      }
    >
      <WalletPageContent {...props} />
    </Suspense>
  );
}
