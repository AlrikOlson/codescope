import { getExtColor } from '../../colors';

interface Props {
  count: number;
  queryTime: number;
  topExts: [string, number][];
  extFilter: string | null;
  onFilterExt: (ext: string | null) => void;
}

export function MetadataStrip({ count, queryTime, topExts, extFilter, onFilterExt }: Props) {
  if (count === 0 && topExts.length === 0) return null;

  return (
    <div className="sw-metadata">
      <span className="sw-metadata-count">{count} results</span>
      <span className="sw-metadata-sep">&middot;</span>
      <span className="sw-metadata-time">{queryTime}ms</span>
      {topExts.length > 0 && (
        <>
          <span className="sw-metadata-sep">&middot;</span>
          <div className="sw-metadata-chips">
            {extFilter && (
              <button
                className="search-filter-chip active clear"
                onClick={() => onFilterExt(null)}
              >
                &times;
              </button>
            )}
            {topExts.map(([ext, c]) => (
              <button
                key={ext}
                className={`search-filter-chip${extFilter === ext ? ' active' : ''}`}
                style={{ '--chip-color': getExtColor(ext) } as React.CSSProperties}
                onClick={() => onFilterExt(extFilter === ext ? null : ext)}
              >
                .{ext}<span className="search-filter-count">{c}</span>
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
