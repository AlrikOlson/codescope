import { getCategoryColor } from '../colors';
import type { DepEntry } from '../types';
import type { GraphNode, DepTree } from './types';

interface Props {
  selectedNode: string;
  nodeData: GraphNode;
  selectedEntry: DepEntry | null;
  depTree: DepTree;
  inspectDepth: number;
  onSetDepth: (d: number) => void;
  onInspectNode: (nodeId: string) => void;
  onClose: () => void;
}

export default function InspectPanel({ selectedNode, nodeData, selectedEntry, depTree, inspectDepth, onSetDepth, onInspectNode, onClose }: Props) {
  return (
    <div className="inspect-overlay">
      <div className="inspect-header">
        <div className="inspect-header-top">
          <span className="inspect-node-dot" style={{ background: getCategoryColor(nodeData.group) }} />
          <span className="inspect-node-name">{selectedNode}</span>
          <button className="inspect-close" onClick={onClose} title="Close">&times;</button>
        </div>
        {nodeData.categoryPath && (
          <div className="inspect-breadcrumb">{nodeData.categoryPath}</div>
        )}
        <div className="inspect-stats">
          <span className="inspect-stat">
            <span className="inspect-stat-label">Group</span>
            <span className="inspect-stat-value" style={{ color: getCategoryColor(nodeData.group) }}>{nodeData.group}</span>
          </span>
          <span className="inspect-stat">
            <span className="inspect-stat-label">Connections</span>
            <span className="inspect-stat-value">{nodeData.depCount}</span>
          </span>
          {selectedEntry && (
            <>
              <span className="inspect-stat">
                <span className="inspect-stat-label">Public deps</span>
                <span className="inspect-stat-value">{selectedEntry.public.length}</span>
              </span>
              <span className="inspect-stat">
                <span className="inspect-stat-label">Private deps</span>
                <span className="inspect-stat-value">{selectedEntry.private.length}</span>
              </span>
            </>
          )}
        </div>
        <div className="inspect-depth-control">
          <span className="inspect-depth-label">Traversal depth</span>
          <div className="inspect-depth-btns">
            {[1, 2, 3].map(d => (
              <button
                key={d}
                className={`inspect-depth-btn${inspectDepth === d ? ' active' : ''}`}
                onClick={() => onSetDepth(d)}
              >
                {d}
              </button>
            ))}
          </div>
        </div>
      </div>

      <div className="inspect-body">
        <div className="inspect-section">
          <div className="inspect-section-title">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M7 17l9.2-9.2M17 17V7H7"/>
            </svg>
            Depends on
            <span className="inspect-section-count">
              {depTree.dependsOn.reduce((sum, l) => sum + l.nodes.length, 0)}
            </span>
          </div>
          {depTree.dependsOn.length === 0 && (
            <div className="inspect-empty">No outgoing dependencies</div>
          )}
          {depTree.dependsOn.map(level => (
            <div key={level.depth} className="inspect-level">
              <div className="inspect-level-label">
                Depth {level.depth}
                <span className="inspect-level-count">{level.nodes.length}</span>
              </div>
              <div className="inspect-nodes">
                {level.nodes.map(n => (
                  <button
                    key={n.id}
                    className="inspect-node-btn"
                    onClick={() => onInspectNode(n.id)}
                    title={n.categoryPath || n.id}
                  >
                    <span className="inspect-node-btn-dot" style={{ background: getCategoryColor(n.group) }} />
                    <span className="inspect-node-btn-name">{n.id}</span>
                    <span className={`inspect-node-btn-type ${n.type}`}>{n.type}</span>
                  </button>
                ))}
              </div>
            </div>
          ))}
        </div>

        <div className="inspect-section">
          <div className="inspect-section-title">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M17 7l-9.2 9.2M7 7v10h10"/>
            </svg>
            Depended by
            <span className="inspect-section-count">
              {depTree.dependedBy.reduce((sum, l) => sum + l.nodes.length, 0)}
            </span>
          </div>
          {depTree.dependedBy.length === 0 && (
            <div className="inspect-empty">No incoming dependencies</div>
          )}
          {depTree.dependedBy.map(level => (
            <div key={level.depth} className="inspect-level">
              <div className="inspect-level-label">
                Depth {level.depth}
                <span className="inspect-level-count">{level.nodes.length}</span>
              </div>
              <div className="inspect-nodes">
                {level.nodes.map(n => (
                  <button
                    key={n.id}
                    className="inspect-node-btn"
                    onClick={() => onInspectNode(n.id)}
                    title={n.categoryPath || n.id}
                  >
                    <span className="inspect-node-btn-dot" style={{ background: getCategoryColor(n.group) }} />
                    <span className="inspect-node-btn-name">{n.id}</span>
                    <span className={`inspect-node-btn-type ${n.type}`}>{n.type}</span>
                  </button>
                ))}
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
