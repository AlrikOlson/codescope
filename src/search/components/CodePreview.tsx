import { useMemo } from 'react';
import { useFileContent } from '../../hooks/useFileContent';
import { tokenizeCode } from '../../syntax';
import { getExtColor } from '../../colors';
import { getFilename, getExt } from '../../utils';

interface Props {
  path: string | null;
}

export function CodePreview({ path }: Props) {
  const { data, loading, error } = useFileContent(path);

  const filename = path ? getFilename(path) : '';
  const ext = path ? getExt(path) : '';
  const color = getExtColor(ext);

  const tokenizedLines = useMemo(() => {
    if (!data?.content) return [];
    return tokenizeCode(data.content, ext);
  }, [data?.content, ext]);

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
