import { useMemo, useRef, useEffect } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useFileContent } from '../../hooks/useFileContent';
import { tokenizeCode } from '../../syntax';
import { getExtColor } from '../../colors';
import { getFilename, getExt } from '../../utils';

interface Props {
  path: string | null;
}

export function CodePreview({ path }: Props) {
  const { data, loading, error } = useFileContent(path);
  const scrollRef = useRef<HTMLDivElement>(null);

  const filename = path ? getFilename(path) : '';
  const ext = path ? getExt(path) : '';
  const color = getExtColor(ext);

  const tokenizedLines = useMemo(() => {
    if (!data?.content) return [];
    return tokenizeCode(data.content, ext);
  }, [data?.content, ext]);

  const virtualizer = useVirtualizer({
    count: tokenizedLines.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 20,
    overscan: 20,
  });

  // Reset scroll to top when file changes
  useEffect(() => {
    if (scrollRef.current) scrollRef.current.scrollTop = 0;
  }, [path]);

  if (!path) {
    return (
      <div className="sw-preview">
        <div className="sw-preview-empty">
          <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1" opacity="0.2">
            <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/>
          </svg>
          <span>Select a file to preview</span>
        </div>
      </div>
    );
  }

  return (
    <div className="sw-preview">
      <div className="preview-header">
        <div className="preview-header-top">
          <span className="preview-ext-badge" style={{ color, borderColor: color }}>{ext}</span>
          <span className="preview-filename">{filename}</span>
          {data && (
            <span className="sw-preview-lines">{data.lines.toLocaleString()} lines</span>
          )}
        </div>
      </div>
      <div ref={scrollRef} className="preview-body">
        {loading && (
          <div className="preview-loading">
            <div className="spinner" />
            Loading...
          </div>
        )}
        {error && (
          <div className="preview-error">Failed to load: {error}</div>
        )}
        {data && !loading && (
          <pre className="preview-code">
            <code style={{ height: virtualizer.getTotalSize(), position: 'relative', display: 'block' }}>
              {virtualizer.getVirtualItems().map(vi => {
                const lineTokens = tokenizedLines[vi.index];
                return (
                  <div
                    key={vi.index}
                    className="preview-line"
                    style={{
                      position: 'absolute',
                      top: vi.start,
                      height: vi.size,
                      width: '100%',
                    }}
                  >
                    <span className="preview-linenum">{vi.index + 1}</span>
                    <span className="preview-linetext">
                      {lineTokens.length === 0
                        ? '\n'
                        : lineTokens.map((tok, j) => (
                            <span key={j} className={`syntax-${tok.type}`}>{tok.text}</span>
                          ))
                      }
                    </span>
                  </div>
                );
              })}
            </code>
          </pre>
        )}
      </div>
    </div>
  );
}
