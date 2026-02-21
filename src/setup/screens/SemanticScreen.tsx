import React from 'react';
import { Brain, AlertCircle, Zap, Feather, Sparkles, Code2, ArrowLeft, ArrowRight } from 'lucide-react';

export interface ModelTier {
  id: string;
  label: string;
  description: string;
  size: string;
  dim: number;
  icon: React.ReactNode;
}

const MODEL_TIERS: ModelTier[] = [
  {
    id: 'lightweight',
    label: 'Lightweight',
    description: 'Fastest, smallest footprint. Good for large monorepos or constrained machines.',
    size: '~23 MB',
    dim: 384,
    icon: <Feather size={14} />,
  },
  {
    id: 'standard',
    label: 'Standard',
    description: 'Best balance of quality and speed. Recommended for most projects.',
    size: '~70 MB',
    dim: 768,
    icon: <Zap size={14} />,
  },
  {
    id: 'quality',
    label: 'Quality',
    description: 'Full-precision model for maximum search accuracy. Larger download.',
    size: '~270 MB',
    dim: 768,
    icon: <Sparkles size={14} />,
  },
  {
    id: 'code',
    label: 'Code',
    description: 'Optimized for source code search. Best for code-heavy projects.',
    size: '~278 MB',
    dim: 768,
    icon: <Code2 size={14} />,
  },
];

interface Props {
  enabled: boolean;
  onEnabledChange: (enabled: boolean) => void;
  selectedModel: string;
  onModelChange: (model: string) => void;
  hasSemantic: boolean;
  onNext: () => void;
  onBack: () => void;
}

export function SemanticScreen({
  enabled, onEnabledChange, selectedModel, onModelChange,
  hasSemantic, onNext, onBack,
}: Props) {
  return (
    <div className="screen">
      <h2><Brain size={17} /> Semantic Search</h2>
      <p className="subtitle">
        An embedding model enables natural language code search —
        find functions by describing what they do, not just their names.
      </p>

      <div className="screen-body">
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
                    Downloads an embedding model and builds vector indexes for each selected project.
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

            <div className={`collapse-container ${enabled ? 'expanded' : ''}`}>
              <div className="model-picker">
                {MODEL_TIERS.map(tier => (
                  <button
                    key={tier.id}
                    className={`model-card ${selectedModel === tier.id ? 'selected' : ''}`}
                    onClick={() => onModelChange(tier.id)}
                  >
                    <div className="model-card-header">
                      <span className="model-card-icon">{tier.icon}</span>
                      <span className="model-card-label">{tier.label}</span>
                      {tier.id === 'standard' && (
                        <span className="model-card-recommended">recommended</span>
                      )}
                    </div>
                    <div className="model-card-desc">{tier.description}</div>
                    <div className="model-card-meta">
                      <span>{tier.size}</span>
                      <span>{tier.dim}-dim</span>
                    </div>
                  </button>
                ))}
              </div>
            </div>
          </>
        )}
      </div>

      <div className="btn-row">
        <button className="btn btn-secondary" onClick={onBack}>
          <ArrowLeft size={13} /> Back
        </button>
        <button className="btn btn-primary" onClick={onNext} autoFocus>
          Continue <ArrowRight size={13} />
        </button>
      </div>
    </div>
  );
}
