import React from 'react';
import { Badge } from '../ui/badge';
import { Button } from '../ui/button';
import { Share2 } from 'lucide-react';

interface Asset {
    symbol: string;
    name: string;
    volume24h: number;
    price: number;
    /** 24h % change, or `null`/`undefined` for an asset with no 24h-ago baseline. */
    change24h: number | null;
    /** New unique holders gained in the last 24h, when known. */
    newHolders24h?: number;
}

interface TopAssetsTableProps {
    assets: Asset[];
}

function buildShareText(asset: Asset): string {
    const changeText = typeof asset.change24h === 'number'
        ? `is ${asset.change24h >= 0 ? 'up' : 'down'} ${Math.abs(asset.change24h)}%`
        : "hasn't moved much";
    return `${asset.symbol} ${changeText} on Stellar today. Price: $${asset.price < 1 ? asset.price.toFixed(4) : asset.price.toLocaleString(undefined, { minimumFractionDigits: 2 })} — via Stellar Insights`;
}

const handleShare = async (asset: Asset) => {
    const shareText = buildShareText(asset);
    const shareUrl = typeof window !== 'undefined' ? window.location.href : undefined;

    // Prefer the native Web Share API where available (mobile browsers, some
    // desktop browsers) so the user gets their OS share sheet rather than
    // being forced straight to X/Twitter.
    if (typeof navigator !== 'undefined' && navigator.share) {
        try {
            await navigator.share({ text: shareText, url: shareUrl });
            return;
        } catch (err) {
            // AbortError means the user cancelled the native share sheet —
            // don't fall back to the X intent link in that case.
            if (err instanceof Error && err.name === 'AbortError') return;
        }
    }

    // Fallback: open an X/Twitter intent link with pre-filled text.
    const intentUrl = `https://twitter.com/intent/tweet?text=${encodeURIComponent(shareText)}${shareUrl ? `&url=${encodeURIComponent(shareUrl)}` : ''}`;
    window.open(intentUrl, '_blank', 'noopener,noreferrer');
};

export const TopAssetsTable: React.FC<TopAssetsTableProps> = ({ assets }) => {
    return (
        <div className="p-6">
            <div className="flex items-center justify-between mb-6">
                <h3 className="text-sm font-bold uppercase tracking-widest text-muted-foreground">Asset Liquidity // Top Movers</h3>
                <Badge variant="outline" className="text-[10px] font-mono border-border/50">LATEST_SNAPSHOT</Badge>
            </div>

            <div className="relative overflow-x-auto">
                <table className="w-full text-sm text-left">
                    <thead>
                        <tr className="border-b border-border/50">
                            <th className="pb-4 font-bold uppercase tracking-widest text-[10px] text-muted-foreground">Asset Pair</th>
                            <th className="pb-4 font-bold uppercase tracking-widest text-[10px] text-muted-foreground text-right">Price</th>
                            <th className="pb-4 font-bold uppercase tracking-widest text-[10px] text-muted-foreground text-right">Change</th>
                            <th className="pb-4 font-bold uppercase tracking-widest text-[10px] text-muted-foreground text-right">New Holders</th>
                            <th className="pb-4 font-bold uppercase tracking-widest text-[10px] text-muted-foreground text-right">Volume (24h)</th>
                            <th className="pb-4 font-bold uppercase tracking-widest text-[10px] text-muted-foreground text-right">Share</th>
                        </tr>
                    </thead>
                    <tbody className="divide-y divide-border/20">
                        {assets.map((asset) => (
                            <tr key={asset.symbol} className="group hover:bg-white/5 transition-colors">
                                <td className="py-4">
                                    <div className="flex items-center gap-3">
                                        <div className="w-8 h-8 rounded-lg bg-accent/10 border border-accent/20 flex items-center justify-center text-accent text-xs font-bold group-hover:glow-accent transition-all">
                                            {asset.symbol.substring(0, 2)}
                                        </div>
                                        <div>
                                            <div className="font-bold tracking-tight">{asset.symbol}</div>
                                            <div className="text-[10px] text-muted-foreground uppercase">{asset.name}</div>
                                        </div>
                                    </div>
                                </td>
                                <td className="py-4 text-right font-mono tabular-nums font-medium">
                                    ${asset.price < 1 ? asset.price.toFixed(4) : asset.price.toLocaleString(undefined, { minimumFractionDigits: 2 })}
                                </td>
                                <td
                                    className={`py-4 text-right font-mono tabular-nums font-bold ${
                                        typeof asset.change24h !== 'number'
                                            ? 'text-muted-foreground'
                                            : asset.change24h >= 0
                                                ? 'text-green-400'
                                                : 'text-red-400'
                                    }`}
                                >
                                    {typeof asset.change24h === 'number'
                                        ? `${asset.change24h > 0 ? '+' : ''}${asset.change24h}%`
                                        : '—'}
                                </td>
                                <td className="py-4 text-right font-mono tabular-nums text-muted-foreground">
                                    {asset.newHolders24h !== undefined
                                        ? `+${asset.newHolders24h.toLocaleString()}`
                                        : '—'}
                                </td>
                                <td className="py-4 text-right font-mono tabular-nums text-muted-foreground">
                                    {new Intl.NumberFormat('en-US', {
                                        style: 'currency',
                                        currency: 'USD',
                                        notation: 'compact'
                                    }).format(asset.volume24h)}
                                </td>
                                <td className="py-4 text-right">
                                    <Button
                                        variant="ghost"
                                        size="icon"
                                        onClick={() => handleShare(asset)}
                                        aria-label={`Share ${asset.symbol} on X/Twitter`}
                                        className="h-8 w-8 hover:bg-accent/20"
                                    >
                                        <Share2 className="h-4 w-4" />
                                    </Button>
                                </td>
                            </tr>
                        ))}
                    </tbody>
                </table>
            </div>
        </div>
    );
};
