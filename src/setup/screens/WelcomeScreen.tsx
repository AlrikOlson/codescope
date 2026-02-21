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
          <Search size={48} strokeWidth={1.5} />
        </div>
        <h1 className="welcome-title">CodeScope</h1>
        {version && <span className="version-badge">v{version}</span>}
        <p className="welcome-subtitle">
          Fast codebase indexer and MCP search server.
          We'll scan for projects, configure semantic search,
          and connect to your tools.
        </p>
        <div className="btn-row" style={{ marginTop: 24 }}>
          <button className="btn btn-primary" onClick={onNext}>
            Get Started <ArrowRight size={13} />
          </button>
        </div>
      </div>
    </div>
  );
}
