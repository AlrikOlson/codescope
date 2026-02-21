import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface RepoInfo {
  path: string;
  name: string;
  ecosystems: string[];
  workspace_info: string | null;
  file_count: number;
}

interface Props {
  repos: RepoInfo[];
  semantic: boolean;
  onNext: () => void;
  onBack: () => void;
}

interface InitResult {
  name: string;
  status: 'pending' | 'running' | 'done' | 'error';
  message: string;
}

export function DoctorScreen({ repos, semantic, onNext, onBack }: Props) {
  const [results, setResults] = useState<InitResult[]>(
    repos.map((r) => ({ name: r.name, status: 'pending', message: '' }))
  );
  const [running, setRunning] = useState(false);
  const [done, setDone] = useState(false);

  const runInit = async () => {
    setRunning(true);
    const newResults = [...results];

    for (let i = 0; i < repos.length; i++) {
      newResults[i] = { ...newResults[i], status: 'running' };
      setResults([...newResults]);

      try {
        const msg = await invoke<string>('init_repo', {
          path: repos[i].path,
          semantic,
        });
        newResults[i] = { ...newResults[i], status: 'done', message: msg };
      } catch (err) {
        newResults[i] = {
          ...newResults[i],
          status: 'error',
          message: String(err),
        };
      }
      setResults([...newResults]);
    }

    setRunning(false);
    setDone(true);
  };

  useEffect(() => {
    if (!done && !running) {
      runInit();
    }
  }, []);

  const statusIcon = (status: string) => {
    switch (status) {
      case 'done': return <span className="check-icon status-pass">&#x2713;</span>;
      case 'error': return <span className="check-icon status-fail">&#x2717;</span>;
      case 'running': return <span className="check-icon" style={{ color: '#4a6cf7' }}>&#x25CF;</span>;
      default: return <span className="check-icon" style={{ color: '#444' }}>&#x25CB;</span>;
    }
  };

  return (
    <div className="screen">
      <h2>Setting Up</h2>
      <p className="subtitle">
        Initializing {repos.length} project{repos.length !== 1 ? 's' : ''}...
      </p>

      <div style={{ marginBottom: '1rem' }}>
        {results.map((r) => (
          <div key={r.name} className="check-item">
            {statusIcon(r.status)}
            <div>
              <div style={{ fontWeight: 500 }}>{r.name}</div>
              {r.message && (
                <div style={{ fontSize: '0.8rem', color: r.status === 'error' ? '#f87171' : '#666' }}>
                  {r.message}
                </div>
              )}
            </div>
          </div>
        ))}
      </div>

      <div className="btn-row">
        <button className="btn btn-secondary" onClick={onBack} disabled={running}>Back</button>
        <button className="btn btn-primary" onClick={onNext} disabled={running}>
          {done ? 'Finish' : 'Setting up...'}
        </button>
      </div>
    </div>
  );
}
