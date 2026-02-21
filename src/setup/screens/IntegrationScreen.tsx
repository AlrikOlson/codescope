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
    invoke<boolean>('check_on_path').then(setOnPath);
  }, []);

  return (
    <div className="screen">
      <h2><Puzzle size={18} /> Integrations</h2>
      <p className="subtitle">
        Configure how CodeScope connects with your development tools.
      </p>

      <div className="toggle-row">
        <div style={{ display: 'flex', alignItems: 'flex-start', gap: '0.75rem', flex: 1 }}>
          <Puzzle size={18} className="toggle-row-icon" style={{ marginTop: 2 }} />
          <div>
            <div className="toggle-label">Claude Code (MCP)</div>
            <div className="toggle-description">
              CodeScope is configured as an MCP server in each project's
              <code>.mcp.json</code> during initialization.
            </div>
          </div>
        </div>
        <span className="toggle-status status-pass">
          <Check size={14} /> Auto
        </span>
      </div>

      <div className="toggle-row">
        <div style={{ display: 'flex', alignItems: 'flex-start', gap: '0.75rem', flex: 1 }}>
          <Terminal size={18} className="toggle-row-icon" style={{ marginTop: 2 }} />
          <div>
            <div className="toggle-label">Shell Completions</div>
            <div className="toggle-description">
              Tab completion for codescope commands. Run{' '}
              <code>codescope completions bash</code> to generate.
            </div>
          </div>
        </div>
        <span className="toggle-status" style={{ color: 'var(--text3)' }}>Manual</span>
      </div>

      <div className="toggle-row">
        <div style={{ display: 'flex', alignItems: 'flex-start', gap: '0.75rem', flex: 1 }}>
          <Globe size={18} className="toggle-row-icon" style={{ marginTop: 2 }} />
          <div>
            <div className="toggle-label">PATH</div>
            <div className="toggle-description">
              {onPath
                ? 'codescope is on your PATH and accessible from any terminal.'
                : 'codescope is not on your PATH. Add ~/.local/bin to your PATH.'}
            </div>
          </div>
        </div>
        <span className={`toggle-status ${onPath ? 'status-pass' : 'status-warn'}`}>
          {onPath
            ? <><Check size={14} /> OK</>
            : <><AlertTriangle size={14} /> Fix</>
          }
        </span>
      </div>

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
