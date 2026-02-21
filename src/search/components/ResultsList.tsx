import { useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { FileIcon } from '../../icons';
import { HighlightedText } from '../../search-utils';
import { MatchTypeBadge } from '../../components/MatchTypeBadge';
import { getExtColor } from '../../colors';
import type { FindResult } from '../../types';

interface Props {
  results: FindResult[];
  activeIdx: number;
  onSetActive: (idx: number) => void;
  onSelect: (path: string) => void;
}

export function ResultsList({ results, activeIdx, onSetActive, onSelect }: Props) {
  const scrollRef = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: results.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (i) => {
      const item = results[i];
      return item?.topMatch ? 72 : 52;
    },
    overscan: 15,
  });

  return (
    <div ref={scrollRef} className="sw-results">
      {results.length === 0 ? (
        <div className="sw-results-empty">No results</div>
      ) : (
        <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
          {virtualizer.getVirtualItems().map(vi => {
            const item = results[vi.index];
            const isActive = vi.index === activeIdx;
            const borderColor = getExtColor(item.ext);

            return (
              <div
                key={item.path}
                className={`search-result-card${isActive ? ' active' : ''}`}
                style={{
                  position: 'absolute',
                  top: vi.start,
                  height: vi.size,
                  width: '100%',
                  '--card-accent': borderColor,
                } as React.CSSProperties}
                onClick={() => onSelect(item.path)}
                onMouseEnter={() => onSetActive(vi.index)}
              >
                <div className="search-card-main">
                  <FileIcon ext={item.ext} size={14} />
                  <div className="search-card-info">
                    <div className="search-card-top">
                      <span className="search-card-filename">
                        <HighlightedText text={item.filename} indices={item.filenameIndices} />
                      </span>
                      <MatchTypeBadge type={item.matchType} count={item.grepCount} />
                    </div>
                    <span className="search-card-path">{item.dir}/</span>
                  </div>
                </div>
                {item.topMatch && (
                  <div className="search-card-snippet">
                    {item.topMatchLine && <span className="snippet-linenum">{item.topMatchLine}</span>}
                    <span className="snippet-text">{item.topMatch.trim()}</span>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
