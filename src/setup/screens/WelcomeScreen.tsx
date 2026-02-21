import React from 'react';

interface Props {
  version: string;
  onNext: () => void;
}

export function WelcomeScreen({ version, onNext }: Props) {
  return (
    <div className="screen">
      <div className="welcome-center">
        <div className="welcome-logo">&#x1F50D;</div>
        <h1>CodeScope</h1>
        <p className="subtitle">
          {version ? `v${version} — ` : ''}Fast codebase indexer and search server
        </p>
        <p className="subtitle" style={{ maxWidth: 420 }}>
          Let's set up CodeScope. We'll scan for projects, configure semantic
          search, and integrate with your tools.
        </p>
        <div className="btn-row">
          <button className="btn btn-primary" onClick={onNext}>
            Get Started
          </button>
        </div>
      </div>
    </div>
  );
}
