import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Check, X, Loader2, Circle, ArrowLeft, ArrowRight } from 'lucide-react';
import type { RepoInfo } from '../SetupWizard';

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
    repos.map((r) => ({ name: r.name, status: 'pending' as const, message: '' }))
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
      case 'done':
        return <Check size={16} className="status-pass" />;
      case 'error':
        return <X size={16} className="status-fail" />;
      case 'running':
        return <Loader2 size={16} className="spinning" style={{ color: 'var(--accent)' }} />;
      default:
        return <Circle size={16} style={{ color: 'var(--text3)' }} />;
    }
  };

  const doneCount = results.filter((r) => r.status === 'done').length;
  const errorCount = results.filter((r) => r.status === 'error').length;

  return (
    <div className="screen">
      <h2>
        {done
          ? (errorCount > 0 ? <X size={18} /> : <Check size={18} />)
          : <Loader2 size={18} className="spinning" />
        }
        {done ? 'Setup Complete' : 'Setting Up'}
      </h2>
      <p className="subtitle">
        {done
          ? `${doneCount} of ${repos.length} project${repos.length !== 1 ? 's' : ''} initialized${errorCount > 0 ? `, ${errorCount} failed` : ''}.`
          : `Initializing ${repos.length} project${repos.length !== 1 ? 's' : ''}...`
        }
      </p>

      <div style={{ marginBottom: '1rem' }}>
        {results.map((r, idx) => (
          <div
            key={r.name}
            className="check-item"
            style={{ '--row-idx': idx } as React.CSSProperties}
          >
            <div className="check-icon">
              {statusIcon(r.status)}
            </div>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div className="check-name">{r.name}</div>
              {r.message && (
                <div className={`check-message ${r.status === 'error' ? 'error' : ''}`}>
                  {r.message}
                </div>
              )}
            </div>
          </div>
        ))}
      </div>

      <div className="btn-row">
        <button className="btn btn-secondary" onClick={onBack} disabled={running}>
          <ArrowLeft size={14} /> Back
        </button>
        <button className="btn btn-primary" onClick={onNext} disabled={running}>
          {done ? 'Finish' : 'Setting up...'} {done && <ArrowRight size={14} />}
        </button>
      </div>
    </div>
  );
}
