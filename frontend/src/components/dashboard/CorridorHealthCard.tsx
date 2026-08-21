import React from 'react';
import { InsightsList } from './InsightsList';

interface Corridor {
  id: string;
  health: number;
  successRate: number;
}

interface CorridorHealthCardProps {
  corridors: Corridor[];
  /** Auto-generated corridor insights (backend#12). Omit to hide the callout entirely. */
  insights?: string[];
  insightsLoading?: boolean;
  insightsError?: string | null;
}

export function CorridorHealthCard({
  corridors,
  insights,
  insightsLoading = false,
  insightsError = null,
}: CorridorHealthCardProps) {
  return (
    <section
      className="col-span-1 bg-white rounded shadow p-4"
      aria-labelledby="corridor-health-heading"
    >
      <h2 id="corridor-health-heading" className="text-sm text-gray-500">
        Active Corridor Health
      </h2>
      <ul className="mt-3 space-y-3" role="list">
        {corridors.map((c) => (
          <li key={c.id} className="flex items-center justify-between">
            <div>
              <div className="font-medium">{c.id}</div>
              <div className="text-sm text-gray-500">
                <span className="sr-only">Success rate: </span>
                Success: {(c.successRate * 100).toFixed(2)}%
              </div>
            </div>
            <div
              className="text-sm font-semibold"
              role="status"
              aria-label={`Health score: ${(c.health * 100).toFixed(0)}%`}
            >
              {(c.health * 100).toFixed(0)}%
            </div>
          </li>
        ))}
      </ul>

      {(insights !== undefined || insightsLoading || insightsError) && (
        <div className="mt-4 pt-4 border-t border-gray-200">
          <h3 className="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-2">
            Insights
          </h3>
          <InsightsList
            insights={insights ?? []}
            isLoading={insightsLoading}
            error={insightsError}
            emptyMessage="No corridor insights available yet."
          />
        </div>
      )}
    </section>
  );
}
