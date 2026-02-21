import React, { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Puzzle, Terminal, Globe, Check, AlertTriangle, ArrowLeft, ArrowRight } from 'lucide-react';

interface Props {
  onNext: () => void;
  onBack: () => void;
}

export function IntegrationScreen({ onNext, onBack }: Props) {
  const [onPath, setOnPath] = useState(true);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    invoke<boolean>('check_on_path')
      .then(setOnPath)
      .catch((e) => console.error('check_on_path failed:', e));
  }, []);

  const copyCompletionsCmd = useCallback(() => {
    try {
      navigator.clipboard.writeText('codescope completions bash');
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Clipboard API may not be available
    }
  }, []);

  return (
    <div className="screen">
      <h2><Puzzle size={17} /> Integrations</h2>
      <p className="subtitle">
        How CodeScope connects with your development environment.
      </p>

      <div className="screen-body">
        <div className="toggle-row" style={{ '--row-idx': 0 } as React.CSSProperties}>
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

        <div className="toggle-row" style={{ '--row-idx': 1 } as React.CSSProperties}>
          <div className="toggle-row-left">
            <Terminal size={16} className="toggle-row-icon" />
            <div>
              <div className="toggle-label">Shell Completions</div>
              <div className="toggle-description">
                Run <code
                  onClick={copyCompletionsCmd}
                  style={{ cursor: 'pointer' }}
                >codescope completions bash</code> to generate tab completions.
                {copied && <span className="copied-badge" style={{ marginLeft: 6 }}>Copied!</span>}
              </div>
            </div>
          </div>
          <span className="toggle-status" style={{ color: 'var(--text3)' }}>Manual</span>
        </div>

        <div className="toggle-row" style={{ '--row-idx': 2 } as React.CSSProperties}>
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
