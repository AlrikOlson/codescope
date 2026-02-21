import React, { useState, useCallback } from 'react';
import { CheckCircle2, Globe, Stethoscope, FolderPlus } from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';

interface Props {
  repoCount: number;
  semantic: boolean;
  semanticModel?: string;
}

export function DoneScreen({ repoCount, semantic, semanticModel }: Props) {
  const [copiedCmd, setCopiedCmd] = useState<string | null>(null);

  const copyCmd = useCallback((cmd: string) => {
    try {
      navigator.clipboard.writeText(cmd);
      setCopiedCmd(cmd);
      setTimeout(() => setCopiedCmd(null), 2000);
    } catch {
      // Clipboard API may not be available
    }
  }, []);

  return (
    <div className="screen">
      <div className="done-layout">
        <div className="done-icon">
          <CheckCircle2 size={52} strokeWidth={1.5} />
        </div>
        <h1 className="done-title">You're all set</h1>
        <p className="done-subtitle">
          {repoCount} project{repoCount !== 1 ? 's' : ''} indexed
          {semantic ? ` with semantic search (${semanticModel ?? 'standard'})` : ''}.
        </p>

        <div className="done-commands">
          <div
            className={`done-command ${copiedCmd === 'codescope web' ? 'copied' : ''}`}
            onClick={() => copyCmd('codescope web')}
          >
            <Globe size={14} />
            <span className="done-command-name">codescope web</span>
            <span className="done-command-sep">&mdash;</span>
            <span className="done-command-desc">Open web UI</span>
            {copiedCmd === 'codescope web' && <span className="copied-badge">Copied!</span>}
          </div>
          <div
            className={`done-command ${copiedCmd === 'codescope doctor' ? 'copied' : ''}`}
            onClick={() => copyCmd('codescope doctor')}
          >
            <Stethoscope size={14} />
            <span className="done-command-name">codescope doctor</span>
            <span className="done-command-sep">&mdash;</span>
            <span className="done-command-desc">Diagnose setup</span>
            {copiedCmd === 'codescope doctor' && <span className="copied-badge">Copied!</span>}
          </div>
          <div
            className={`done-command ${copiedCmd === 'codescope init <path>' ? 'copied' : ''}`}
            onClick={() => copyCmd('codescope init <path>')}
          >
            <FolderPlus size={14} />
            <span className="done-command-name">codescope init &lt;path&gt;</span>
            <span className="done-command-sep">&mdash;</span>
            <span className="done-command-desc">Add a project</span>
            {copiedCmd === 'codescope init <path>' && <span className="copied-badge">Copied!</span>}
          </div>
        </div>

        <div className="btn-row" style={{ marginTop: 32 }}>
          <button
            className="btn btn-primary"
            onClick={() => getCurrentWindow().close()}
            autoFocus
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
