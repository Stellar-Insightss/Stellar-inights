'use client';

import React, { useState, useCallback, useRef } from 'react';
import { Save, ArrowLeft } from 'lucide-react';
import { PortfolioReorder, PortfolioItem } from '@/components/PortfolioReorder';

const INITIAL_PORTFOLIO: PortfolioItem[] = [
  {
    id: '1',
    title: 'DeFi Dashboard',
    description: 'Real-time analytics for Stellar DeFi protocols',
    imageUrl: '',
    projectUrl: '',
    order: 0,
  },
  {
    id: '2',
    title: 'NFT Marketplace',
    description: 'Marketplace for Stellar-based digital assets',
    imageUrl: '',
    projectUrl: '',
    order: 1,
  },
  {
    id: '3',
    title: 'Payment Gateway',
    description: 'Cross-border payment integration using Stellar anchors',
    imageUrl: '',
    projectUrl: '',
    order: 2,
  },
];

export default function ProfileEditPage() {
  const [portfolio, setPortfolio] = useState<PortfolioItem[]>(INITIAL_PORTFOLIO);
  const [isSaving, setIsSaving] = useState(false);
  const [lastSaved, setLastSaved] = useState<Date | null>(null);
  const [displayName, setDisplayName] = useState('');
  const [bio, setBio] = useState('');
  const saveTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const persistOrder = useCallback(async (items: PortfolioItem[]) => {
    setIsSaving(true);
    // Optimistic: local state already updated
    // Debounce the API call
    if (saveTimeoutRef.current) {
      clearTimeout(saveTimeoutRef.current);
    }
    saveTimeoutRef.current = setTimeout(async () => {
      try {
        // In production this would be: await fetch(`/api/creators/${id}`, { method: 'PATCH', body: JSON.stringify({ portfolio: items }) })
        await new Promise((resolve) => setTimeout(resolve, 300));
        setLastSaved(new Date());
      } finally {
        setIsSaving(false);
      }
    }, 500);
  }, []);

  const handleReorder = useCallback(
    (items: PortfolioItem[]) => {
      setPortfolio(items);
      persistOrder(items);
    },
    [persistOrder],
  );

  const handleDelete = useCallback(
    (id: string) => {
      const updated = portfolio
        .filter((item) => item.id !== id)
        .map((item, i) => ({ ...item, order: i }));
      setPortfolio(updated);
      persistOrder(updated);
    },
    [portfolio, persistOrder],
  );

  const handleAdd = useCallback(
    (newItem: Omit<PortfolioItem, 'id' | 'order'>) => {
      const item: PortfolioItem = {
        ...newItem,
        id: `portfolio-${Date.now()}`,
        order: portfolio.length,
      };
      const updated = [...portfolio, item];
      setPortfolio(updated);
      persistOrder(updated);
    },
    [portfolio, persistOrder],
  );

  const handleSaveProfile = async () => {
    setIsSaving(true);
    try {
      // In production: PATCH /api/creators/:id
      await new Promise((resolve) => setTimeout(resolve, 500));
      setLastSaved(new Date());
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div className="max-w-4xl mx-auto space-y-8 p-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <a
            href="../profile"
            className="p-2 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
            aria-label="Back to profile"
          >
            <ArrowLeft size={20} />
          </a>
          <div>
            <h1 className="text-2xl font-bold text-gray-800 dark:text-gray-200">
              Edit Profile
            </h1>
            {lastSaved && (
              <p className="text-xs text-gray-500 dark:text-gray-400">
                Last saved {lastSaved.toLocaleTimeString()}
              </p>
            )}
          </div>
        </div>
        <button
          onClick={handleSaveProfile}
          disabled={isSaving}
          className="flex items-center gap-2 px-4 py-2 bg-blue-500 hover:bg-blue-600 disabled:bg-blue-300 text-white rounded-lg transition-colors font-medium"
        >
          <Save size={18} />
          {isSaving ? 'Saving...' : 'Save Profile'}
        </button>
      </div>

      {/* Profile Info Section */}
      <div className="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6 space-y-4">
        <h2 className="text-lg font-semibold text-gray-800 dark:text-gray-200">
          Profile Information
        </h2>
        <div className="space-y-4">
          <div>
            <label
              htmlFor="displayName"
              className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
            >
              Display Name
            </label>
            <input
              id="displayName"
              type="text"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              placeholder="Your display name"
              className="w-full px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-gray-200 focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
          </div>
          <div>
            <label
              htmlFor="bio"
              className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1"
            >
              Bio
            </label>
            <textarea
              id="bio"
              value={bio}
              onChange={(e) => setBio(e.target.value)}
              placeholder="Tell others about yourself..."
              rows={3}
              className="w-full px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-gray-200 focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
          </div>
        </div>
      </div>

      {/* Portfolio Reorder Section */}
      <div className="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
        <PortfolioReorder
          items={portfolio}
          onReorder={handleReorder}
          onDelete={handleDelete}
          onAdd={handleAdd}
          isSaving={isSaving}
        />
      </div>
    </div>
  );
}
