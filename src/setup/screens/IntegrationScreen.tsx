import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Puzzle, Terminal, Globe, Check, AlertTriangle, ArrowLeft, ArrowRight } from 'lucide-react';

interface Props {
  onNext: () => void;
  onBack: () => void;
}

export function IntegrationScreen({ onNext, onBack }: Props) {
  const [onPath, setOnPath] = useState(true);

  useEffect(() => {
    invoke<boolean>('check_on_path')
      .then(setOnPath)
      .catch((e) => console.error('check_on_path failed:', e));
  }, []);

  return (
    <div className="screen">
      <h2><Puzzle size={17} /> Integrations</h2>
      <p className="subtitle">
        How CodeScope connects with your development environment.
      </p>

      <div className="toggle-row">
        <div className="toggle-row-left">
          <Puzzle size={16} className="toggle-row-icon" />
          <div>
            <div className="toggle-label">Claude Code (MCP)</div>
            <div className="toggle-description">
              Each project gets a <code>.mcp.json</code> configured automatically during initialization.
            </div>
          </div>
        </div>
        <span className="toggle-status status-pass">
          <Check size={13} /> Auto
        </span>
      </div>

      <div className="toggle-row">
        <div className="toggle-row-left">
          <Terminal size={16} className="toggle-row-icon" />
          <div>
            <div className="toggle-label">Shell Completions</div>
            <div className="toggle-description">
              Run <code>codescope completions bash</code> to generate tab completions.
            </div>
          </div>
        </div>
        <span className="toggle-status" style={{ color: 'var(--text3)' }}>Manual</span>
      </div>

      <div className="toggle-row">
        <div className="toggle-row-left">
          <Globe size={16} className="toggle-row-icon" />
          <div>
            <div className="toggle-label">PATH</div>
            <div className="toggle-description">
              {onPath
                ? 'codescope is accessible from any terminal.'
                : 'Not on PATH. Add ~/.local/bin to your shell profile.'}
            </div>
          </div>
        </div>
        <span className={`toggle-status ${onPath ? 'status-pass' : 'status-warn'}`}>
          {onPath
            ? <><Check size={13} /> OK</>
            : <><AlertTriangle size={13} /> Fix</>
          }
        </span>
      </div>

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
