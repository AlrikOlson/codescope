import React from 'react';
import { Brain, AlertCircle, Cpu, ArrowLeft, ArrowRight } from 'lucide-react';

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
      <h2><Brain size={17} /> Semantic Search</h2>
      <p className="subtitle">
        A BERT model enables natural language code search —
        find functions by describing what they do, not just their names.
      </p>

      {!hasSemantic && (
        <div className="toggle-row unavailable-banner">
          <div className="toggle-row-left">
            <AlertCircle size={16} className="toggle-row-icon" style={{ color: 'var(--red)' }} />
            <div>
              <div className="toggle-label" style={{ color: 'var(--red)' }}>Not Available</div>
              <div className="toggle-description">
                This binary was built without semantic search.
                Rebuild with <code>--features semantic</code> to enable.
              </div>
            </div>
          </div>
        </div>
      )}

      {hasSemantic && (
        <>
          <div className="toggle-row">
            <div className="toggle-row-left">
              <Brain size={16} className="toggle-row-icon" />
              <div>
                <div className="toggle-label">Enable semantic search</div>
                <div className="toggle-description">
                  Downloads MiniLM-L6 (~23 MB) and builds vector embeddings for each selected project.
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
            <div className="toggle-row-left">
              <Cpu size={16} className="toggle-row-icon" />
              <div>
                <div className="toggle-label">Model</div>
                <div className="toggle-description">
                  all-MiniLM-L6-v2 — 384 dimensions, fast inference, general-purpose
                </div>
              </div>
            </div>
            <span className="toggle-status" style={{ color: 'var(--accent)' }}>Default</span>
          </div>
        </>
      )}

      <div className="btn-row">
        <button className="btn btn-secondary" onClick={onBack}>
          <ArrowLeft size={13} /> Back
        </button>
        <button className="btn btn-primary" onClick={onNext}>
          Continue <ArrowRight size={13} />
        </button>
      </div>
    </div>
  );
}
