import { getCategoryColor } from '../colors';
import type { MultiInspectData } from './types';

interface Props {
  data: MultiInspectData;
  onInspectNode: (nodeId: string) => void;
  onClose: () => void;
}

export default function MultiInspectPanel({ data, onInspectNode, onClose }: Props) {
  return (
    <div className="inspect-overlay multi">
      <div className="inspect-header">
        <div className="inspect-header-top">
          <span className="inspect-multi-icon">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <circle cx="6" cy="6" r="3"/><circle cx="18" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="18" r="3"/>
              <path d="M9 6h6M6 9v6M18 9v6M9 18h6"/>
            </svg>
          </span>
          <span className="inspect-node-name">{data.modules.length} Modules Selected</span>
          <button className="inspect-close" onClick={onClose} title="Close">&times;</button>
        </div>
        <div className="inspect-multi-summary">
          {data.connections.length} connection{data.connections.length !== 1 ? 's' : ''}
          {' · '}
          {data.sharedDeps.length} shared dep{data.sharedDeps.length !== 1 ? 's' : ''}
        </div>
      </div>

      <div className="inspect-body">
        <div className="inspect-section">
          <div className="inspect-section-title">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M22 11.08V12a10 10 0 11-5.93-9.14"/><path d="M22 4L12 14.01l-3-3"/>
            </svg>
            Selected Modules
            <span className="inspect-section-count">{data.modules.length}</span>
          </div>
          <div className="inspect-nodes">
            {data.modules.map(m => (
              <button
                key={m.id}
                className="inspect-node-btn"
                onClick={() => onInspectNode(m.id)}
                title={m.categoryPath || m.id}
              >
                <span className="inspect-node-btn-dot" style={{ background: getCategoryColor(m.group) }} />
                <span className="inspect-node-btn-name">{m.id}</span>
                <span className="inspect-node-btn-meta">{m.depCount} deps</span>
              </button>
            ))}
          </div>
        </div>

        {data.connections.length > 0 && (
          <div className="inspect-section">
            <div className="inspect-section-title">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M5 12h14M12 5l7 7-7 7"/>
              </svg>
              Connections
              <span className="inspect-section-count">{data.connections.length}</span>
            </div>
            <div className="inspect-connections">
              {data.connections.map((c, i) => (
                <div key={i} className={`inspect-connection ${c.type}`}>
                  <div className="inspect-connection-header">
                    <button className="inspect-conn-node" onClick={() => onInspectNode(c.from)}>
                      {c.from}
                    </button>
                    <span className={`inspect-conn-arrow ${c.type}`}>
                      {c.type === 'indirect' ? '···→' : '→'}
                    </span>
                    <button className="inspect-conn-node" onClick={() => onInspectNode(c.to)}>
                      {c.to}
                    </button>
                    <span className={`inspect-node-btn-type ${c.type}`}>{c.type}</span>
                  </div>
                  {c.type === 'indirect' && c.path.length > 2 && (
                    <div className="inspect-connection-path">
                      {c.path.map((step, si) => (
                        <span key={si}>
                          {si > 0 && <span className="inspect-path-arrow">→</span>}
                          <button className="inspect-path-node" onClick={() => onInspectNode(step)}>
                            {step}
                          </button>
                        </span>
                      ))}
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>
        )}

        {data.sharedDeps.length > 0 && (
          <div className="inspect-section">
            <div className="inspect-section-title">
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <circle cx="12" cy="12" r="10"/><path d="M8 12h8M12 8v8"/>
              </svg>
              Shared Dependencies
              <span className="inspect-section-count">{data.sharedDeps.length}</span>
            </div>
            <div className="inspect-nodes">
              {data.sharedDeps.map(d => (
                <button
                  key={d.id}
                  className="inspect-node-btn"
                  onClick={() => onInspectNode(d.id)}
                >
                  <span className="inspect-node-btn-dot" style={{ background: getCategoryColor(d.group) }} />
                  <span className="inspect-node-btn-name">{d.id}</span>
                  <span className="inspect-node-btn-meta">{d.dependedByCount}/{data.modules.length} depend</span>
                </button>
              ))}
            </div>
          </div>
        )}

        {data.connections.length === 0 && data.sharedDeps.length === 0 && (
          <div className="inspect-empty">No direct or indirect connections found between selected modules</div>
        )}
      </div>
    </div>
  );
}
