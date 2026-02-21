import React from 'react';

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
      <h2>Semantic Search</h2>
      <p className="subtitle">
        CodeScope uses a BERT model to understand code semantically. This enables
        natural language code search beyond simple text matching.
      </p>

      {!hasSemantic && (
        <div className="toggle-row" style={{ borderColor: '#f8717155' }}>
          <div>
            <div className="toggle-label" style={{ color: '#f87171' }}>Not Available</div>
            <div className="toggle-description">
              This binary was built without semantic search support.
              Rebuild with <code>--features semantic</code> to enable.
            </div>
          </div>
        </div>
      )}

      {hasSemantic && (
        <>
          <div className="toggle-row">
            <div>
              <div className="toggle-label">Enable semantic search</div>
              <div className="toggle-description">
                Downloads the MiniLM-L6 model (~23 MB) and builds embeddings for your code.
              </div>
            </div>
            <input
              type="checkbox"
              checked={enabled}
              onChange={(e) => onEnabledChange(e.target.checked)}
              style={{ accentColor: '#4a6cf7', width: 20, height: 20 }}
            />
          </div>

          <div className="toggle-row">
            <div>
              <div className="toggle-label">Model</div>
              <div className="toggle-description">
                sentence-transformers/all-MiniLM-L6-v2 (384 dimensions, fast, general-purpose)
              </div>
            </div>
            <span style={{ color: '#4a6cf7', fontSize: '0.85rem' }}>Default</span>
          </div>
        </>
      )}

      <div className="btn-row">
        <button className="btn btn-secondary" onClick={onBack}>Back</button>
        <button className="btn btn-primary" onClick={onNext}>Next</button>
      </div>
    </div>
  );
}
