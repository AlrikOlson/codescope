import React, { useState, useEffect, useRef, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  Check, X, Loader2, Circle, ArrowLeft, ArrowRight,
  Brain, Cpu, Layers, FileCode2, AlertTriangle,
} from 'lucide-react';
import type { RepoInfo } from '../SetupWizard';

interface Props {
  repos: RepoInfo[];
  semantic: boolean;
  semanticModel: string;
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
  status: string;
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

/* SVG ring geometry */
const RING_R = 50;
const RING_C = 2 * Math.PI * RING_R;
const RING_R2 = 60;
const RING_C2 = 2 * Math.PI * RING_R2;

export function DoctorScreen({ repos, semantic, semanticModel, onNext, onBack }: Props) {
  const finishRef = useRef<HTMLButtonElement>(null);
  const [results, setResults] = useState<InitResult[]>(
    repos.map((r) => ({ name: r.name, status: 'pending' as const, message: '' }))
  );
  const [running, setRunning] = useState(false);
  const [done, setDone] = useState(false);
  const [semState, setSemState] = useState<SemanticState>({
    active: false, currentRepo: '', status: '',
    totalChunks: 0, totalBatches: 0, completedBatches: 0,
    device: '', completedRepos: 0, totalRepos: 0,
  });
  const completedSemRepos = useRef(new Set<string>());

  /* ---- init logic (unchanged) ---- */
  const runInit = async () => {
    setRunning(true);
    const updated = [...results];

    for (let i = 0; i < repos.length; i++) {
      updated[i] = { ...updated[i], status: 'running' };
      setResults([...updated]);
      try {
        const msg = await invoke<string>('init_repo', { path: repos[i].path });
        updated[i] = { ...updated[i], status: 'done', message: msg };
      } catch (err) {
        updated[i] = { ...updated[i], status: 'error', message: String(err) };
      }
      setResults([...updated]);
    }

    if (semantic) {
      const paths = repos
        .filter((_, i) => updated[i].status === 'done')
        .map((r) => r.path);
      if (paths.length > 0) {
        completedSemRepos.current = new Set();
        setSemState((s) => ({ ...s, active: true, totalRepos: paths.length, completedRepos: 0 }));
        invoke('build_semantic_async', { paths, model: semanticModel }).catch((err) =>
          console.error('build_semantic_async failed:', err)
        );
        return;
      }
    }
    setRunning(false);
    setDone(true);
  };

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
          setTimeout(() => { setRunning(false); setDone(true); }, 0);
        }
        const isNewRepo = p.repo !== prev.currentRepo;
        const completedBatches = isNewRepo
          ? p.completed_batches
          : Math.max(prev.completedBatches, p.completed_batches);
        return {
          ...prev, currentRepo: p.repo, status: p.status,
          totalChunks: p.total_chunks, totalBatches: p.total_batches,
          completedBatches, device: p.device, completedRepos: newCompleted.size,
        };
      });
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => { if (!done && !running) runInit(); }, []);
  useEffect(() => { if (done) finishRef.current?.focus(); }, [done]);

  /* ---- derived state ---- */
  const doneCount = results.filter((r) => r.status === 'done').length;
  const errorCount = results.filter((r) => r.status === 'error').length;
  const completedCount = doneCount + errorCount;
  const configDone = completedCount === repos.length;
  const configProgress = repos.length > 0 ? completedCount / repos.length : 0;

  const semPercent =
    semState.totalBatches > 0
      ? Math.round((semState.completedBatches / semState.totalBatches) * 100)
      : 0;

  const semStatusLabel = () => {
    switch (semState.status) {
      case 'extracting': return 'Extracting code chunks...';
      case 'embedding':
        return semState.device ? `Embedding on ${semState.device}...` : 'Embedding...';
      case 'ready': return 'Complete';
      case 'failed': return 'Failed';
      default: return 'Starting...';
    }
  };

  const totalFiles = useMemo(
    () => repos.reduce((sum, r) => sum + r.file_count, 0), [repos],
  );
  const ecosystems = useMemo(
    () => [...new Set(repos.flatMap((r) => r.ecosystems))], [repos],
  );

  const phase: 'config' | 'semantic' | 'complete' =
    !configDone ? 'config'
    : semState.active && !done ? 'semantic'
    : 'complete';

  const runningRepo = results.find((r) => r.status === 'running')?.name;

  const ringWrapClass = [
    'doctor-ring-wrap',
    done ? 'all-done' : '',
    configDone && !done ? 'config-done' : '',
  ].filter(Boolean).join(' ');

  const ringStroke =
    configDone
      ? errorCount > 0 ? 'var(--yellow)' : 'var(--green)'
      : 'url(#ringGrad)';

  const statusIcon = (status: string) => {
    switch (status) {
      case 'done':  return <Check size={13} className="status-pass" />;
      case 'error': return <X size={13} className="status-fail" />;
      case 'running':
        return <Loader2 size={13} className="spinning" style={{ color: 'var(--accent)' }} />;
      default:
        return <Circle size={13} style={{ color: 'var(--text3)' }} />;
    }
  };

  return (
    <div className="screen">
      <div className="screen-body">
      <div className="doctor-layout">
        {/* ---- Hero: Progress Ring ---- */}
        <div className="doctor-hero">
          <div className={ringWrapClass}>
            <svg className="doctor-ring" viewBox="0 0 140 140" overflow="visible">
              <defs>
                <linearGradient id="ringGrad" x1="0%" y1="0%" x2="100%" y2="100%">
                  <stop offset="0%" stopColor="var(--neon-cyan)" />
                  <stop offset="100%" stopColor="var(--accent2)" />
                </linearGradient>
                <linearGradient id="ringGrad2" x1="0%" y1="0%" x2="100%" y2="0%">
                  <stop offset="0%" stopColor="var(--mauve)" />
                  <stop offset="100%" stopColor="var(--neon-cyan)" />
                </linearGradient>
                <filter id="ringGlow">
                  <feGaussianBlur in="SourceGraphic" stdDeviation="2.5" result="blur" />
                  <feMerge>
                    <feMergeNode in="blur" />
                    <feMergeNode in="SourceGraphic" />
                  </feMerge>
                </filter>
              </defs>

              {/* Outer semantic track (always rendered, fades in) */}
              <circle cx="70" cy="70" r={RING_R2} fill="none"
                stroke="var(--surface2)" strokeWidth="3"
                opacity={semantic && configDone ? 0.6 : 0}
                className="doctor-ring-track-outer"
              />
              {/* Outer semantic arc */}
              {semantic && configDone && (
                <circle cx="70" cy="70" r={RING_R2} fill="none"
                  stroke="url(#ringGrad2)" strokeWidth="3" strokeLinecap="round"
                  strokeDasharray={RING_C2}
                  strokeDashoffset={RING_C2 * (1 - semPercent / 100)}
                  transform="rotate(-90 70 70)"
                  className="doctor-ring-arc"
                  filter="url(#ringGlow)"
                />
              )}

              {/* Inner config track */}
              <circle cx="70" cy="70" r={RING_R} fill="none"
                stroke="var(--surface2)" strokeWidth="5"
              />
              {/* Inner config arc */}
              <circle cx="70" cy="70" r={RING_R} fill="none"
                stroke={ringStroke} strokeWidth="5" strokeLinecap="round"
                strokeDasharray={RING_C}
                strokeDashoffset={RING_C * (1 - configProgress)}
                transform="rotate(-90 70 70)"
                className="doctor-ring-arc"
                filter="url(#ringGlow)"
              />
            </svg>

            {/* Center label */}
            <div className="doctor-ring-center">
              {done ? (
                errorCount > 0
                  ? <AlertTriangle size={26} className="doctor-done-warn" />
                  : <Check size={28} className="doctor-done-check" />
              ) : (
                <>
                  <span className="doctor-ring-num">{doneCount}</span>
                  <span className="doctor-ring-denom">/{repos.length}</span>
                </>
              )}
            </div>

            {/* Completion ripple */}
            {done && <span className="doctor-ripple" />}
          </div>

          {/* Phase indicator */}
          <div className="doctor-phase">
            <span className={`doctor-phase-pip ${phase}`} />
            <span className="doctor-phase-label">
              {phase === 'config' && 'Configuring Projects'}
              {phase === 'semantic' && 'Semantic Indexing'}
              {phase === 'complete' && (errorCount > 0 ? 'Completed with Errors' : 'All Systems Go')}
            </span>
          </div>

          {/* Activity line */}
          <p className="doctor-activity">
            {runningRepo
              ? `Initializing ${runningRepo}...`
              : phase === 'semantic'
                ? semStatusLabel()
                : done
                  ? `${doneCount} project${doneCount !== 1 ? 's' : ''} initialized`
                  : 'Preparing...'}
          </p>
        </div>

        {/* ---- Project Cards ---- */}
        <div className={`doctor-grid ${configDone && !done ? 'dimmed' : ''}`}>
          {results.map((r, idx) => (
            <div
              key={r.name}
              className={`doctor-card ${r.status}`}
              style={{ '--card-idx': idx } as React.CSSProperties}
              title={r.message || repos[idx]?.path}
            >
              <div className="doctor-card-icon">{statusIcon(r.status)}</div>
              <div className="doctor-card-body">
                <span className="doctor-card-name">{r.name}</span>
                <div className="doctor-card-tags">
                  {repos[idx]?.ecosystems.slice(0, 2).map((e) => (
                    <span key={e} className="doctor-card-eco">{e}</span>
                  ))}
                  {repos[idx]?.file_count > 0 && (
                    <span className="doctor-card-fcount">
                      {repos[idx].file_count.toLocaleString()} files
                    </span>
                  )}
                </div>
              </div>
              {r.status === 'error' && r.message && (
                <span className="doctor-card-err" title={r.message}>
                  <AlertTriangle size={11} />
                </span>
              )}
            </div>
          ))}
        </div>

        {/* ---- Semantic Progress ---- */}
        {semState.active && !done && configDone && (
          <div className="doctor-semantic">
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
                  className={`semantic-bar-fill ${semState.status === 'extracting' ? 'indeterminate' : ''}`}
                  style={semState.status !== 'extracting' ? { width: `${semPercent}%` } : undefined}
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
          </div>
        )}

        {/* ---- Stats Pills ---- */}
        <div className="doctor-stats">
          <div className="doctor-stat" style={{ '--stat-idx': 0 } as React.CSSProperties}>
            <Layers size={12} />
            <span className="doctor-stat-val">{repos.length}</span>
            <span className="doctor-stat-lbl">projects</span>
          </div>
          <div className="doctor-stat" style={{ '--stat-idx': 1 } as React.CSSProperties}>
            <FileCode2 size={12} />
            <span className="doctor-stat-val">{totalFiles.toLocaleString()}</span>
            <span className="doctor-stat-lbl">files</span>
          </div>
          {ecosystems.slice(0, 4).map((e, i) => (
            <div key={e} className="doctor-stat eco"
              style={{ '--stat-idx': i + 2 } as React.CSSProperties}>
              <span className="doctor-stat-val">{e}</span>
            </div>
          ))}
          {semantic && (
            <div className="doctor-stat"
              style={{ '--stat-idx': Math.min(ecosystems.length, 4) + 2 } as React.CSSProperties}>
              <Brain size={12} />
              <span className="doctor-stat-val">{semanticModel}</span>
            </div>
          )}
          {semState.device && (
            <div className="doctor-stat"
              style={{ '--stat-idx': Math.min(ecosystems.length, 4) + 3 } as React.CSSProperties}>
              <Cpu size={12} />
              <span className="doctor-stat-val">{semState.device}</span>
            </div>
          )}
        </div>
      </div>
      </div>

      <div className="btn-row">
        <button className="btn btn-secondary" onClick={onBack} disabled={running}>
          <ArrowLeft size={13} /> Back
        </button>
        <button ref={finishRef} className="btn btn-primary" onClick={onNext} disabled={running}>
          {done ? <>Finish <ArrowRight size={13} /></> : 'Initializing...'}
        </button>
      </div>
    </div>
  );
}
