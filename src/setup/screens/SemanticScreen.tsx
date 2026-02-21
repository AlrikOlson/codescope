import React from 'react';
import { Brain, AlertCircle, ArrowLeft, ArrowRight } from 'lucide-react';

interface Props {
  enabled: boolean;
  onEnabledChange: (enabled: boolean) => void;
  hasSemantic: boolean;
  onNext: () => void;
  onBack: () => void;
}

export function SemanticScreen({ enabled, onEnabledChange, hasSemantic, onNext, onBack }: Props) {
  return (
    <div className="screen">
      <h2><Brain size={18} /> Semantic Search</h2>
      <p className="subtitle">
        CodeScope uses a BERT model to understand code semantically, enabling
        natural language search beyond simple text matching.
      </p>

      {!hasSemantic && (
        <div className="toggle-row unavailable-banner">
          <div style={{ display: 'flex', alignItems: 'flex-start', gap: '0.75rem' }}>
            <AlertCircle size={18} style={{ color: 'var(--red)', flexShrink: 0, marginTop: 2 }} />
            <div>
              <div className="toggle-label" style={{ color: 'var(--red)' }}>Not Available</div>
              <div className="toggle-description">
                This binary was built without semantic search support.
                Rebuild with <code>--features semantic</code> to enable.
              </div>
            </div>
          </div>
        </div>
      )}

      {hasSemantic && (
        <>
          <div className="toggle-row">
            <div style={{ display: 'flex', alignItems: 'flex-start', gap: '0.75rem', flex: 1 }}>
              <Brain size={18} className="toggle-row-icon" style={{ marginTop: 2 }} />
              <div>
                <div className="toggle-label">Enable semantic search</div>
                <div className="toggle-description">
                  Downloads the MiniLM-L6 model (~23 MB) and builds embeddings for your code.
                </div>
              </div>
            </div>
            <label className="toggle-switch">
              <input
                type="checkbox"
                checked={enabled}
                onChange={(e) => onEnabledChange(e.target.checked)}
              />
              <span className="slider" />
            </label>
          </div>

          <div className="toggle-row">
            <div style={{ display: 'flex', alignItems: 'flex-start', gap: '0.75rem', flex: 1 }}>
              <div className="toggle-row-icon" style={{ width: 18 }} />
              <div>
                <div className="toggle-label">Model</div>
                <div className="toggle-description">
                  sentence-transformers/all-MiniLM-L6-v2 (384 dimensions, fast, general-purpose)
                </div>
              </div>
            </div>
            <span className="toggle-status" style={{ color: 'var(--accent)' }}>Default</span>
          </div>
        </>
      )}

      <div className="btn-row">
        <button className="btn btn-secondary" onClick={onBack}>
          <ArrowLeft size={14} /> Back
        </button>
        <button className="btn btn-primary" onClick={onNext}>
          Next <ArrowRight size={14} />
        </button>
      </div>
    </div>
  );
}
