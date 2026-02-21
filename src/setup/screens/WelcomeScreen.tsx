import React from 'react';
import { Search, ArrowRight } from 'lucide-react';

interface Props {
  version: string;
  onNext: () => void;
}

export function WelcomeScreen({ version, onNext }: Props) {
  return (
    <div className="screen">
      <div className="welcome-center">
        <div className="welcome-logo">
          <Search size={56} strokeWidth={1.5} />
        </div>
        <h1 className="welcome-title">CodeScope</h1>
        {version && <span className="version-badge">v{version}</span>}
        <p className="subtitle">
          Fast codebase indexer and MCP search server.
          Let's scan for projects, configure semantic search,
          and integrate with your tools.
        </p>
        <div className="btn-row">
          <button className="btn btn-primary" onClick={onNext}>
            Get Started <ArrowRight size={14} />
          </button>
        </div>
      </div>
    </div>
  );
}
