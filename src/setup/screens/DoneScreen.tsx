import React from 'react';

interface Props {
  repoCount: number;
  semantic: boolean;
}

export function DoneScreen({ repoCount, semantic }: Props) {
  return (
    <div className="screen">
      <div className="welcome-center">
        <div className="welcome-logo" style={{ fontSize: '4rem' }}>&#x2713;</div>
        <h1>You're all set!</h1>
        <p className="subtitle">
          {repoCount} project{repoCount !== 1 ? 's' : ''} indexed
          {semantic ? ' with semantic search' : ''}.
        </p>
        <div style={{ textAlign: 'left', maxWidth: 400, margin: '1rem auto' }}>
          <p style={{ color: '#888', marginBottom: '1rem' }}>Get started:</p>
          <div style={{ fontFamily: 'monospace', fontSize: '0.9rem', lineHeight: 2 }}>
            <div><span style={{ color: '#4a6cf7' }}>codescope web</span> &mdash; Open the web UI</div>
            <div><span style={{ color: '#4a6cf7' }}>codescope doctor</span> &mdash; Check your setup</div>
            <div><span style={{ color: '#4a6cf7' }}>codescope init &lt;path&gt;</span> &mdash; Add another project</div>
          </div>
        </div>
      </div>
    </div>
  );
}
