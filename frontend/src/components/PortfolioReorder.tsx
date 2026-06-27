'use client';

import React, { useState, useCallback, useRef, useEffect } from 'react';
import { GripVertical, Trash2, Plus, Image, ExternalLink, Check, X } from 'lucide-react';

export interface PortfolioItem {
  id: string;
  title: string;
  description: string;
  imageUrl?: string;
  projectUrl?: string;
  order: number;
}

interface PortfolioReorderProps {
  items: PortfolioItem[];
  onReorder: (items: PortfolioItem[]) => void;
  onDelete?: (id: string) => void;
  onAdd?: (item: Omit<PortfolioItem, 'id' | 'order'>) => void;
  isSaving?: boolean;
}

export const PortfolioReorder: React.FC<PortfolioReorderProps> = ({
  items,
  onReorder,
  onDelete,
  onAdd,
  isSaving = false,
}) => {
  const [localItems, setLocalItems] = useState<PortfolioItem[]>(items);
  const [draggedIndex, setDraggedIndex] = useState<number | null>(null);
  const [dragOverIndex, setDragOverIndex] = useState<number | null>(null);
  const [focusedIndex, setFocusedIndex] = useState<number | null>(null);
  const [showAddForm, setShowAddForm] = useState(false);
  const [newTitle, setNewTitle] = useState('');
  const [newDescription, setNewDescription] = useState('');
  const [newImageUrl, setNewImageUrl] = useState('');
  const [newProjectUrl, setNewProjectUrl] = useState('');
  const itemRefs = useRef<(HTMLDivElement | null)[]>([]);

  useEffect(() => {
    setLocalItems(items);
  }, [items]);

  const reorder = useCallback(
    (fromIndex: number, toIndex: number) => {
      if (fromIndex === toIndex) return;
      const updated = [...localItems];
      const [moved] = updated.splice(fromIndex, 1);
      updated.splice(toIndex, 0, moved);
      const reordered = updated.map((item, i) => ({ ...item, order: i }));
      setLocalItems(reordered);
      onReorder(reordered);
    },
    [localItems, onReorder],
  );

  const handleDragStart = (e: React.DragEvent, index: number) => {
    setDraggedIndex(index);
    e.dataTransfer.effectAllowed = 'move';
    if (e.currentTarget instanceof HTMLElement) {
      e.currentTarget.style.opacity = '0.5';
    }
  };

  const handleDragEnd = (e: React.DragEvent) => {
    if (e.currentTarget instanceof HTMLElement) {
      e.currentTarget.style.opacity = '1';
    }
    setDraggedIndex(null);
    setDragOverIndex(null);
  };

  const handleDragOver = (e: React.DragEvent, index: number) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    setDragOverIndex(index);
  };

  const handleDrop = (e: React.DragEvent, targetIndex: number) => {
    e.preventDefault();
    if (draggedIndex !== null) {
      reorder(draggedIndex, targetIndex);
    }
    setDraggedIndex(null);
    setDragOverIndex(null);
  };

  const handleKeyDown = (e: React.KeyboardEvent, index: number) => {
    if (e.key === 'ArrowUp' && index > 0) {
      e.preventDefault();
      reorder(index, index - 1);
      setFocusedIndex(index - 1);
      setTimeout(() => itemRefs.current[index - 1]?.focus(), 0);
    } else if (e.key === 'ArrowDown' && index < localItems.length - 1) {
      e.preventDefault();
      reorder(index, index + 1);
      setFocusedIndex(index + 1);
      setTimeout(() => itemRefs.current[index + 1]?.focus(), 0);
    }
  };

  const handleAddItem = () => {
    if (!newTitle.trim()) return;
    onAdd?.({
      title: newTitle.trim(),
      description: newDescription.trim(),
      imageUrl: newImageUrl.trim() || undefined,
      projectUrl: newProjectUrl.trim() || undefined,
    });
    setNewTitle('');
    setNewDescription('');
    setNewImageUrl('');
    setNewProjectUrl('');
    setShowAddForm(false);
  };

  return (
    <div className="w-full" role="region" aria-label="Portfolio items">
      <div className="flex items-center justify-between mb-4">
        <div>
          <h3 className="text-lg font-semibold text-gray-800 dark:text-gray-200">
            Portfolio Items
          </h3>
          <p className="text-sm text-gray-500 dark:text-gray-400">
            Drag to reorder or use arrow keys on focused items
          </p>
        </div>
        {isSaving && (
          <span className="text-sm text-blue-600 dark:text-blue-400 animate-pulse">
            Saving...
          </span>
        )}
      </div>

      <div
        className="space-y-2 min-h-[200px] rounded-lg"
        role="list"
        aria-label="Reorderable portfolio items"
      >
        {localItems.length === 0 ? (
          <div className="flex items-center justify-center h-48 text-gray-400 dark:text-gray-500 border-2 border-dashed border-gray-200 dark:border-gray-700 rounded-lg">
            <p>No portfolio items yet. Add one to get started.</p>
          </div>
        ) : (
          localItems.map((item, index) => (
            <div
              key={item.id}
              ref={(el) => { itemRefs.current[index] = el; }}
              draggable
              tabIndex={0}
              role="listitem"
              aria-label={`${item.title} — position ${index + 1} of ${localItems.length}. Use arrow keys to reorder.`}
              onDragStart={(e) => handleDragStart(e, index)}
              onDragEnd={handleDragEnd}
              onDragOver={(e) => handleDragOver(e, index)}
              onDragLeave={() => setDragOverIndex(null)}
              onDrop={(e) => handleDrop(e, index)}
              onKeyDown={(e) => handleKeyDown(e, index)}
              onFocus={() => setFocusedIndex(index)}
              onBlur={() => setFocusedIndex(null)}
              className={`flex items-center gap-3 p-4 bg-white dark:bg-gray-800 rounded-lg border-2 transition-all cursor-move group ${
                dragOverIndex === index
                  ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20 scale-[1.02]'
                  : focusedIndex === index
                  ? 'border-blue-400 ring-2 ring-blue-200 dark:ring-blue-800'
                  : 'border-gray-200 dark:border-gray-700 hover:border-gray-300 dark:hover:border-gray-600'
              }`}
            >
              <div
                className="flex-shrink-0 cursor-grab active:cursor-grabbing p-1"
                aria-hidden="true"
              >
                <GripVertical size={20} className="text-gray-400" />
              </div>

              {item.imageUrl && (
                <div className="flex-shrink-0 w-12 h-12 rounded-md overflow-hidden bg-gray-100 dark:bg-gray-700">
                  <img
                    src={item.imageUrl}
                    alt={item.title}
                    className="w-full h-full object-cover"
                  />
                </div>
              )}

              <div className="flex-1 min-w-0">
                <p className="font-medium text-gray-800 dark:text-gray-200 truncate">
                  {item.title}
                </p>
                {item.description && (
                  <p className="text-sm text-gray-500 dark:text-gray-400 truncate">
                    {item.description}
                  </p>
                )}
              </div>

              {item.projectUrl && (
                <a
                  href={item.projectUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="flex-shrink-0 p-2 text-gray-400 hover:text-blue-500 transition-colors"
                  aria-label={`Visit ${item.title} project`}
                  onClick={(e) => e.stopPropagation()}
                >
                  <ExternalLink size={16} />
                </a>
              )}

              <span className="flex-shrink-0 text-xs text-gray-500 bg-gray-100 dark:bg-gray-700 dark:text-gray-400 px-2 py-1 rounded">
                #{index + 1}
              </span>

              {onDelete && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    onDelete(item.id);
                  }}
                  className="flex-shrink-0 opacity-0 group-hover:opacity-100 focus:opacity-100 transition-opacity p-2 hover:bg-red-50 dark:hover:bg-red-900/20 rounded-lg text-red-600"
                  aria-label={`Delete ${item.title}`}
                >
                  <Trash2 size={18} />
                </button>
              )}
            </div>
          ))
        )}
      </div>

      {onAdd && (
        <div className="mt-4">
          {!showAddForm ? (
            <button
              onClick={() => setShowAddForm(true)}
              className="w-full flex items-center justify-center gap-2 px-4 py-3 bg-blue-500 hover:bg-blue-600 text-white rounded-lg transition-colors font-medium"
              aria-label="Add new portfolio item"
            >
              <Plus size={20} />
              Add Portfolio Item
            </button>
          ) : (
            <div className="space-y-3 p-4 bg-gray-50 dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700">
              <input
                type="text"
                value={newTitle}
                onChange={(e) => setNewTitle(e.target.value)}
                placeholder="Project title *"
                className="w-full px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-gray-200 focus:outline-none focus:ring-2 focus:ring-blue-500"
                autoFocus
                aria-label="Project title"
              />
              <textarea
                value={newDescription}
                onChange={(e) => setNewDescription(e.target.value)}
                placeholder="Description"
                rows={2}
                className="w-full px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-gray-200 focus:outline-none focus:ring-2 focus:ring-blue-500"
                aria-label="Project description"
              />
              <div className="grid grid-cols-2 gap-3">
                <input
                  type="url"
                  value={newImageUrl}
                  onChange={(e) => setNewImageUrl(e.target.value)}
                  placeholder="Image URL"
                  className="px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-gray-200 focus:outline-none focus:ring-2 focus:ring-blue-500"
                  aria-label="Image URL"
                />
                <input
                  type="url"
                  value={newProjectUrl}
                  onChange={(e) => setNewProjectUrl(e.target.value)}
                  placeholder="Project URL"
                  className="px-4 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-800 dark:text-gray-200 focus:outline-none focus:ring-2 focus:ring-blue-500"
                  aria-label="Project URL"
                />
              </div>
              <div className="flex gap-2 justify-end">
                <button
                  onClick={() => {
                    setShowAddForm(false);
                    setNewTitle('');
                    setNewDescription('');
                    setNewImageUrl('');
                    setNewProjectUrl('');
                  }}
                  className="px-4 py-2 bg-gray-200 dark:bg-gray-600 hover:bg-gray-300 dark:hover:bg-gray-500 text-gray-800 dark:text-gray-200 rounded-lg transition-colors"
                  aria-label="Cancel"
                >
                  <X size={16} className="inline mr-1" />
                  Cancel
                </button>
                <button
                  onClick={handleAddItem}
                  disabled={!newTitle.trim()}
                  className="px-4 py-2 bg-green-500 hover:bg-green-600 disabled:bg-gray-300 dark:disabled:bg-gray-600 text-white rounded-lg transition-colors"
                  aria-label="Add item"
                >
                  <Check size={16} className="inline mr-1" />
                  Add
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
};

export default PortfolioReorder;
