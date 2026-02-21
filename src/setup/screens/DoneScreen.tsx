import React from 'react';
import { CheckCircle2, Globe, Stethoscope, FolderPlus } from 'lucide-react';

interface Props {
  repoCount: number;
  semantic: boolean;
}

export function DoneScreen({ repoCount, semantic }: Props) {
  return (
    <div className="screen">
      <div className="welcome-center">
        <div className="done-icon">
          <CheckCircle2 size={64} strokeWidth={1.5} />
        </div>
        <h1 className="welcome-title">You're all set</h1>
        <p className="subtitle">
          {repoCount} project{repoCount !== 1 ? 's' : ''} indexed
          {semantic ? ' with semantic search' : ''}.
        </p>
        <div className="done-commands">
          <div className="done-command">
            <Globe size={15} />
            <span className="done-command-name">codescope web</span>
            <span className="done-command-desc">Open the web UI</span>
          </div>
          <div className="done-command">
            <Stethoscope size={15} />
            <span className="done-command-name">codescope doctor</span>
            <span className="done-command-desc">Check your setup</span>
          </div>
          <div className="done-command">
            <FolderPlus size={15} />
            <span className="done-command-name">codescope init &lt;path&gt;</span>
            <span className="done-command-desc">Add another project</span>
          </div>
        </div>
      </div>
    </div>
  );
}
