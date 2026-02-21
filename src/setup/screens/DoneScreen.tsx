import React from 'react';
import { CheckCircle2, Globe, Stethoscope, FolderPlus } from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';

interface Props {
  repoCount: number;
  semantic: boolean;
}

export function DoneScreen({ repoCount, semantic }: Props) {
  return (
    <div className="screen">
      <div className="done-layout">
        <div className="done-icon">
          <CheckCircle2 size={52} strokeWidth={1.5} />
        </div>
        <h1 className="done-title">You're all set</h1>
        <p className="done-subtitle">
          {repoCount} project{repoCount !== 1 ? 's' : ''} indexed
          {semantic ? ' with semantic search enabled' : ''}.
        </p>

        <div className="done-commands">
          <div className="done-command">
            <Globe size={14} />
            <span className="done-command-name">codescope web</span>
            <span className="done-command-sep">&mdash;</span>
            <span className="done-command-desc">Open web UI</span>
          </div>
          <div className="done-command">
            <Stethoscope size={14} />
            <span className="done-command-name">codescope doctor</span>
            <span className="done-command-sep">&mdash;</span>
            <span className="done-command-desc">Diagnose setup</span>
          </div>
          <div className="done-command">
            <FolderPlus size={14} />
            <span className="done-command-name">codescope init &lt;path&gt;</span>
            <span className="done-command-sep">&mdash;</span>
            <span className="done-command-desc">Add a project</span>
          </div>
        </div>

        <div className="btn-row" style={{ marginTop: 32 }}>
          <button
            className="btn btn-primary"
            onClick={() => getCurrentWindow().close()}
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
