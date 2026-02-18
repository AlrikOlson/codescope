import { useMemo } from 'react';
import { useFileContent } from './hooks/useFileContent';
import { tokenizeCode } from './syntax';
import { getExtColor } from './colors';
import { getFilename, getExt } from './utils';
import type { Manifest } from './types';
import './styles/preview.css';

interface Props {
  path: string | null;
  manifest: Manifest;
  selected: Set<string>;
  onClose: () => void;
  onToggleFile: (path: string) => void;
  isMaximized?: boolean;
  onToggleMaximize?: () => void;
}

export default function FilePreview({ path, manifest, selected, onClose, onToggleFile, isMaximized, onToggleMaximize }: Props) {
  const { data, loading, error } = useFileContent(path);

  const filename = path ? getFilename(path) : '';
  const ext = path ? getExt(path) : '';
  const color = getExtColor(ext);
  const isSelected = path ? selected.has(path) : false;

  const tokenizedLines = useMemo(() => {
    if (!data?.content) return [];
    return tokenizeCode(data.content, ext);
  }, [data?.content, ext]);

  if (!path) return null;

  return (
    <div className={`preview-panel${isMaximized ? ' maximized' : ''}`}>
      <div className="preview-header">
        <div className="preview-header-top">
          <span className="preview-ext-badge" style={{ color, borderColor: color }}>{ext}</span>
          <span className="preview-filename">{filename}</span>
          <div className="preview-header-btns">
            {onToggleMaximize && (
              <button className="preview-maximize" onClick={onToggleMaximize} title={isMaximized ? 'Restore' : 'Maximize'}>
                {isMaximized ? '\u2292' : '\u229E'}
              </button>
            )}
            <button className="preview-close" onClick={onClose}>&times;</button>
          </div>
        </div>
        <div className="preview-meta">
          {data && (
            <>
              <span>{data.lines.toLocaleString()} lines</span>
              <span className="preview-dot"> · </span>
              <span>{formatBytes(data.size)}</span>
              {data.truncated && (
                <>
                  <span className="preview-dot"> · </span>
                  <span className="preview-truncated">truncated to 512KB</span>
                </>
              )}
            </>
          )}
        </div>
        <div className="preview-toolbar">
          <button
            className={`preview-select-btn${isSelected ? ' selected' : ''}`}
            onClick={() => onToggleFile(path)}
          >
            {isSelected ? '✓ Selected' : '+ Select'}
          </button>
        </div>
      </div>

      <div className="preview-body">
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
            <code>
              {tokenizedLines.map((lineTokens, i) => (
                <div key={i} className="preview-line">
                  <span className="preview-linenum">{i + 1}</span>
                  <span className="preview-linetext">
                    {lineTokens.length === 0
                      ? '\n'
                      : lineTokens.map((tok, j) => (
                          <span key={j} className={`syntax-${tok.type}`}>{tok.text}</span>
                        ))
                    }
                  </span>
                </div>
              ))}
            </code>
          </pre>
        )}
      </div>
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}
