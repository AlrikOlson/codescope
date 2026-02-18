import { useState, useCallback, useMemo, useRef, useEffect } from 'react';
import { getFilename, getExt } from './utils';
import { getExtColor } from './colors';
import { estimateTokens, formatTokenCount } from './tokenCount';
import { copyToClipboard, buildSmartContext, buildFullContents, buildPathsOnly } from './copyLogic';
import type { Manifest } from './types';
import './styles/context-panel.css';

const MODEL_LIMITS: Record<string, { name: string; tokens: number }> = {
  'claude-200k': { name: 'Claude 200K', tokens: 200_000 },
  'gpt4-128k': { name: 'GPT-4 128K', tokens: 128_000 },
  'claude-100k': { name: 'Claude 100K', tokens: 100_000 },
  'custom-50k': { name: 'Custom 50K', tokens: 50_000 },
};

const MODEL_KEYS = Object.keys(MODEL_LIMITS);

interface Props {
  selected: Set<string>;
  manifest: Manifest;
  searchQuery: string | null;
  onClear: () => void;
  onRemoveFile: (path: string) => void;
}

export default function ContextPanel({
  selected,
  manifest,
  searchQuery,
  onClear,
  onRemoveFile,
}: Props) {
  const [modelKey, setModelKey] = useState<string>(MODEL_KEYS[0]);
  const [orderedPaths, setOrderedPaths] = useState<string[]>([]);
  const [dragIdx, setDragIdx] = useState<number | null>(null);
  const [dropIdx, setDropIdx] = useState<number | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [copying, setCopying] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);

  const limit = MODEL_LIMITS[modelKey]?.tokens ?? 200_000;

  // Sync orderedPaths with selected set: add new items to end, remove deleted
  useEffect(() => {
    setOrderedPaths(prev => {
      const selectedArr = [...selected];
      const kept = prev.filter(p => selected.has(p));
      const existing = new Set(kept);
      const added = selectedArr.filter(p => !existing.has(p));
      return [...kept, ...added];
    });
  }, [selected]);

  // Build a size lookup from manifest
  const sizeMap = useMemo(() => {
    const map = new Map<string, number>();
    for (const files of Object.values(manifest)) {
      for (const f of files) {
        if (!map.has(f.path)) map.set(f.path, f.size || 0);
      }
    }
    return map;
  }, [manifest]);

  const totalTokens = useMemo(() => {
    let bytes = 0;
    for (const p of orderedPaths) {
      bytes += sizeMap.get(p) || 0;
    }
    return estimateTokens(bytes);
  }, [orderedPaths, sizeMap]);

  const budgetRatio = limit > 0 ? totalTokens / limit : 0;
  const budgetLevel = budgetRatio > 0.9 ? 'danger' : budgetRatio > 0.6 ? 'warn' : 'ok';
  const budgetPercent = Math.min(100, budgetRatio * 100);

  function showToast(msg: string) {
    setToast(msg);
    setTimeout(() => setToast(null), 2000);
  }

  // -- Drag and drop handlers --
  const handleDragStart = useCallback((idx: number) => {
    setDragIdx(idx);
  }, []);

  const handleDragOver = useCallback((e: React.DragEvent, idx: number) => {
    e.preventDefault();
    setDropIdx(idx);
  }, []);

  const handleDrop = useCallback(() => {
    if (dragIdx !== null && dropIdx !== null && dragIdx !== dropIdx) {
      setOrderedPaths(prev => {
        const next = [...prev];
        const [moved] = next.splice(dragIdx, 1);
        next.splice(dropIdx > dragIdx ? dropIdx - 1 : dropIdx, 0, moved);
        return next;
      });
    }
    setDragIdx(null);
    setDropIdx(null);
  }, [dragIdx, dropIdx]);

  const handleDragEnd = useCallback(() => {
    setDragIdx(null);
    setDropIdx(null);
  }, []);

  // -- Copy handlers --
  const handleCopySmartContext = useCallback(async () => {
    if (selected.size === 0) return;
    setCopying(true);
    try {
      const result = await buildSmartContext(selected, manifest, searchQuery, limit);
      await copyToClipboard(result.text);
      showToast(result.toast);
    } catch (e) {
      showToast(`Copy failed: ${e instanceof Error ? e.message : 'unknown error'}`);
    } finally {
      setCopying(false);
    }
  }, [selected, manifest, searchQuery, limit]);

  const handleCopyFullContents = useCallback(async () => {
    if (selected.size === 0) return;
    setCopying(true);
    try {
      const result = await buildFullContents(selected, manifest);
      await copyToClipboard(result.text);
      showToast(result.toast);
    } catch (e) {
      showToast(`Copy failed: ${e instanceof Error ? e.message : 'unknown error'}`);
    } finally {
      setCopying(false);
    }
  }, [selected, manifest]);

  const handleCopyPathsOnly = useCallback(() => {
    if (selected.size === 0) return;
    const text = buildPathsOnly(selected, manifest);
    copyToClipboard(text).then(() => showToast(`Copied ${selected.size} file paths`));
  }, [selected, manifest]);

  return (
    <div className="context-panel">
      {/* Header */}
      <div className="context-header">
        <span className="context-title">Context</span>
        {selected.size > 0 && (
          <span className="context-file-count">{selected.size}</span>
        )}
        <div className="context-header-spacer" />
        <select
          className="context-model-select"
          value={modelKey}
          onChange={e => setModelKey(e.target.value)}
        >
          {MODEL_KEYS.map(k => (
            <option key={k} value={k}>{MODEL_LIMITS[k].name}</option>
          ))}
        </select>
        {selected.size > 0 && (
          <button className="context-clear-btn" onClick={onClear}>Clear</button>
        )}
      </div>

      {/* File List */}
      <div className="context-file-list" ref={listRef}>
        {orderedPaths.length === 0 ? (
          <div className="context-empty">
            <div className="context-empty-icon">{ }</div>
            <span>Select files to build context</span>
          </div>
        ) : (
          orderedPaths.map((path, idx) => {
            const ext = getExt(path);
            const filename = getFilename(path);
            const size = sizeMap.get(path) || 0;
            const tokens = estimateTokens(size);
            const color = getExtColor(ext);

            return (
              <div key={path}>
                {dropIdx === idx && dragIdx !== null && dragIdx !== idx && (
                  <div className="context-drop-indicator" />
                )}
                <div
                  className={`context-file-row${dragIdx === idx ? ' dragging' : ''}`}
                  draggable
                  onDragStart={() => handleDragStart(idx)}
                  onDragOver={(e) => handleDragOver(e, idx)}
                  onDrop={handleDrop}
                  onDragEnd={handleDragEnd}
                  title={path}
                >
                  <span className="context-drag-handle">&#10303;</span>
                  <span className="context-ext-dot" style={{ backgroundColor: color }} />
                  <span className="context-file-name">{filename}</span>
                  <span className="context-file-tokens">{formatTokenCount(tokens)}</span>
                  <button
                    className="context-remove-btn"
                    onClick={() => onRemoveFile(path)}
                    title="Remove from context"
                  >
                    &times;
                  </button>
                </div>
              </div>
            );
          })
        )}
        {/* Drop indicator at the end of the list */}
        {dropIdx !== null && dropIdx >= orderedPaths.length && dragIdx !== null && (
          <div className="context-drop-indicator" />
        )}
      </div>

      {/* Token Budget Bar */}
      {selected.size > 0 && (
        <div className="context-budget">
          <div className="context-budget-bar">
            <div
              className={`context-budget-fill level-${budgetLevel}`}
              style={{ width: `${budgetPercent}%` }}
            />
          </div>
          <div className="context-budget-label">
            <span>{formatTokenCount(totalTokens)}</span>
            <span>{formatTokenCount(limit)}</span>
          </div>
        </div>
      )}

      {/* Export Buttons */}
      {selected.size > 0 && (
        <div className="context-export">
          <button onClick={handleCopySmartContext} disabled={copying}>
            Smart
          </button>
          <button onClick={handleCopyFullContents} disabled={copying}>
            Full
          </button>
          <button onClick={handleCopyPathsOnly} disabled={copying}>
            Paths
          </button>
        </div>
      )}

      {/* Toast */}
      <div className={`context-toast${toast ? ' show' : ''}`}>{toast}</div>
    </div>
  );
}
