import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

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
      <h2>Integrations</h2>
      <p className="subtitle">
        Configure how CodeScope connects with your tools.
      </p>

      <div className="toggle-row">
        <div>
          <div className="toggle-label">Claude Code (MCP)</div>
          <div className="toggle-description">
            CodeScope will be configured as an MCP server in each project's .mcp.json
            when initialized.
          </div>
        </div>
        <span className="status-pass">Auto</span>
      </div>

      <div className="toggle-row">
        <div>
          <div className="toggle-label">Shell Completions</div>
          <div className="toggle-description">
            Tab completion for codescope commands. Run{' '}
            <code>codescope completions bash</code> to generate.
          </div>
        </div>
        <span style={{ color: '#888', fontSize: '0.85rem' }}>Manual</span>
      </div>

      <div className="toggle-row">
        <div>
          <div className="toggle-label">PATH</div>
          <div className="toggle-description">
            {onPath
              ? 'codescope is on your PATH and accessible from any terminal.'
              : 'codescope is not on your PATH. Add ~/.local/bin to your PATH.'}
          </div>
        </div>
        <span className={onPath ? 'status-pass' : 'status-warn'}>
          {onPath ? 'OK' : 'Fix'}
        </span>
      </div>

      <div className="btn-row">
        <button className="btn btn-secondary" onClick={onBack}>Back</button>
        <button className="btn btn-primary" onClick={onNext}>Next</button>
      </div>
    </div>
  );
}
