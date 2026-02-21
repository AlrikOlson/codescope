import React, { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Check, X, Loader2, Circle, ArrowLeft, ArrowRight, Brain } from 'lucide-react';
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

interface SemanticProgress {
  repo: string;
  status: string; // "extracting", "embedding", "ready", "failed"
  total_chunks: number;
  total_batches: number;
  completed_batches: number;
  device: string;
}

interface SemanticState {
  active: boolean;
  currentRepo: string;
  status: string;
  totalChunks: number;
  totalBatches: number;
  completedBatches: number;
  device: string;
  completedRepos: number;
  totalRepos: number;
}

export function DoctorScreen({ repos, semantic, onNext, onBack }: Props) {
  const [results, setResults] = useState<InitResult[]>(
    repos.map((r) => ({ name: r.name, status: 'pending' as const, message: '' }))
  );
  const [running, setRunning] = useState(false);
  const [done, setDone] = useState(false);
  const [semState, setSemState] = useState<SemanticState>({
    active: false,
    currentRepo: '',
    status: '',
    totalChunks: 0,
    totalBatches: 0,
    completedBatches: 0,
    device: '',
    completedRepos: 0,
    totalRepos: 0,
  });
  const completedSemRepos = useRef(new Set<string>());

  const runInit = async () => {
    setRunning(true);
    const updated = [...results];

    // Phase 1: Fast config init for all repos (no semantic)
    for (let i = 0; i < repos.length; i++) {
      updated[i] = { ...updated[i], status: 'running' };
      setResults([...updated]);

      try {
        const msg = await invoke<string>('init_repo', {
          path: repos[i].path,
        });
        updated[i] = { ...updated[i], status: 'done', message: msg };
      } catch (err) {
        updated[i] = { ...updated[i], status: 'error', message: String(err) };
      }
      setResults([...updated]);
    }

    // Phase 2: Async semantic index (if enabled)
    if (semantic) {
      const paths = repos
        .filter((_, i) => updated[i].status === 'done')
        .map((r) => r.path);

      if (paths.length > 0) {
        completedSemRepos.current = new Set();
        setSemState((s) => ({
          ...s,
          active: true,
          totalRepos: paths.length,
          completedRepos: 0,
        }));

        // Fire-and-forget — progress comes via events
        invoke('build_semantic_async', { paths }).catch((err) =>
          console.error('build_semantic_async failed:', err)
        );
        return; // Don't setDone yet — semantic progress listener handles that
      }
    }

    setRunning(false);
    setDone(true);
  };

  // Listen for semantic progress events
  useEffect(() => {
    const unlisten = listen<SemanticProgress>('semantic-progress', (event) => {
      const p = event.payload;
      setSemState((prev) => {
        const newCompleted = new Set(completedSemRepos.current);
        if (p.status === 'ready' || p.status === 'failed') {
          newCompleted.add(p.repo);
          completedSemRepos.current = newCompleted;
        }
        const allDone = newCompleted.size >= prev.totalRepos && prev.totalRepos > 0;
        if (allDone) {
          // Defer state updates to avoid batching issues
          setTimeout(() => {
            setRunning(false);
            setDone(true);
          }, 0);
        }
        return {
          ...prev,
          currentRepo: p.repo,
          status: p.status,
          totalChunks: p.total_chunks,
          totalBatches: p.total_batches,
          completedBatches: p.completed_batches,
          device: p.device,
          completedRepos: newCompleted.size,
        };
      });
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (!done && !running) runInit();
  }, []);

  const statusIcon = (status: string) => {
    switch (status) {
      case 'done':
        return <Check size={15} className="status-pass" />;
      case 'error':
        return <X size={15} className="status-fail" />;
      case 'running':
        return <Loader2 size={15} className="spinning" style={{ color: 'var(--accent)' }} />;
      default:
        return <Circle size={15} style={{ color: 'var(--text3)' }} />;
    }
  };

  const doneCount = results.filter((r) => r.status === 'done').length;
  const errorCount = results.filter((r) => r.status === 'error').length;
  const configDone = results.every((r) => r.status === 'done' || r.status === 'error');

  // Semantic progress percentage
  const semPercent =
    semState.totalBatches > 0
      ? Math.round((semState.completedBatches / semState.totalBatches) * 100)
      : 0;

  const semStatusLabel = () => {
    switch (semState.status) {
      case 'extracting':
        return 'Extracting code chunks...';
      case 'embedding':
        return semState.device
          ? `Embedding on ${semState.device}...`
          : 'Embedding...';
      case 'ready':
        return 'Complete';
      case 'failed':
        return 'Failed';
      default:
        return 'Starting...';
    }
  };

  return (
    <div className="screen">
      <h2>
        {done
          ? (errorCount > 0 ? <AlertIcon /> : <Check size={17} />)
          : <Loader2 size={17} className="spinning" />
        }
        {done ? 'Initialization Complete' : 'Initializing Projects'}
      </h2>
      <p className="subtitle">
        {done
          ? `${doneCount} of ${repos.length} initialized${errorCount > 0 ? ` — ${errorCount} failed` : ' successfully'}.`
          : configDone && semState.active
            ? 'Building semantic search indexes...'
            : `Setting up ${repos.length} project${repos.length !== 1 ? 's' : ''}...`
        }
      </p>

      <div className="check-list">
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

      {/* Semantic progress section */}
      {semState.active && !done && configDone && (
        <div className="semantic-progress">
          <div className="semantic-header">
            <Brain size={14} style={{ color: 'var(--accent)' }} />
            <span className="semantic-label">Semantic Index</span>
            <span className="semantic-repo">{semState.currentRepo}</span>
            {semState.totalRepos > 1 && (
              <span className="semantic-counter">
                {semState.completedRepos + 1}/{semState.totalRepos}
              </span>
            )}
          </div>
          <div className="semantic-bar-track">
            <div
              className="semantic-bar-fill"
              style={{ width: `${semState.status === 'extracting' ? 5 : semPercent}%` }}
            />
          </div>
          <div className="semantic-detail">
            <span>{semStatusLabel()}</span>
            {semState.status === 'embedding' && semState.totalBatches > 0 && (
              <span>
                {semState.completedBatches}/{semState.totalBatches} batches ({semPercent}%)
              </span>
            )}
            {semState.totalChunks > 0 && semState.status === 'extracting' && (
              <span>{semState.totalChunks.toLocaleString()} chunks</span>
            )}
          </div>
        </div>
      )}

      <div className="btn-row">
        <button className="btn btn-secondary" onClick={onBack} disabled={running}>
          <ArrowLeft size={13} /> Back
        </button>
        <button className="btn btn-primary" onClick={onNext} disabled={running}>
          {done ? <>Finish <ArrowRight size={13} /></> : 'Initializing...'}
        </button>
      </div>
    </div>
  );
}

function AlertIcon() {
  return <X size={17} style={{ color: 'var(--yellow)' }} />;
}
