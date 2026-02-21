import React, { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import {
  FolderOpen, FolderPlus, ArrowLeft, ArrowRight, Loader2,
  AlertCircle, CheckCircle2, ChevronDown, ChevronRight, Brain,
  RefreshCw, Check,
} from 'lucide-react';
import type { RepoInfo } from '../SetupWizard';

interface Props {
  selectedRepos: RepoInfo[];
  onSelectedReposChange: (repos: RepoInfo[]) => void;
  registeredPaths: string[];
  onNext: () => void;
  onBack: () => void;
}

export function RepoPickerScreen({ selectedRepos, onSelectedReposChange, registeredPaths, onNext, onBack }: Props) {
  const [allRepos, setAllRepos] = useState<RepoInfo[]>([]);
  const [scanning, setScanning] = useState(false);
  const [scanned, setScanned] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [readyExpanded, setReadyExpanded] = useState(false);
  const [scanCount, setScanCount] = useState(0);
  const hasAutoSelected = useRef(false);

  const readyRepos = allRepos.filter(r => r.status === 'ready');
  const selectableRepos = allRepos.filter(r => r.status !== 'ready');

  const scanDirs = async () => {
    setScanning(true);
    setError(null);
    try {
      const dirs = await invoke<string[]>('get_scan_dirs');
      const repos = await invoke<RepoInfo[]>('scan_for_repos', { dirs, registered: registeredPaths });
      setAllRepos(repos);

      // Auto-select repos that need work (stale + needs_setup) on first scan
      if (!hasAutoSelected.current) {
        hasAutoSelected.current = true;
        const needsWork = repos.filter(r => r.status === 'stale' || r.status === 'needs_setup');
        if (needsWork.length > 0 && selectedRepos.length === 0) {
          onSelectedReposChange(needsWork);
        }
      }
      setScanned(true);
      setScanCount(c => c + 1);
    } catch (err) {
      console.error('Scan failed:', err);
      setError(String(err));
      setScanned(true);
    } finally {
      setScanning(false);
    }
  };

  useEffect(() => {
    if (!scanned) scanDirs();
  }, []);

  const toggleRepo = (repo: RepoInfo) => {
    const exists = selectedRepos.some((r) => r.path === repo.path);
    if (exists) {
      onSelectedReposChange(selectedRepos.filter((r) => r.path !== repo.path));
    } else {
      onSelectedReposChange([...selectedRepos, repo]);
    }
  };

  const selectAll = () => {
    if (selectedRepos.length === selectableRepos.length) {
      onSelectedReposChange([]);
    } else {
      onSelectedReposChange([...selectableRepos]);
    }
  };

  const addDirectory = async () => {
    try {
      const selected = await open({ directory: true, multiple: false });
      if (selected) {
        const repo = await invoke<RepoInfo>('detect_project', { path: selected });
        if (!allRepos.some((r) => r.path === repo.path)) {
          setAllRepos([...allRepos, repo]);
        }
        if (!selectedRepos.some((r) => r.path === repo.path)) {
          onSelectedReposChange([...selectedRepos, repo]);
        }
      }
    } catch (err) {
      console.error('Add directory failed:', err);
      setError(String(err));
    }
  };

  const staleCount = allRepos.filter(r => r.status === 'stale').length;
  const needsSetupCount = allRepos.filter(r => r.status === 'needs_setup').length;
  const newCount = allRepos.filter(r => r.status === 'new').length;

  const subtitle = scanning
    ? 'Scanning your filesystem for projects...'
    : !scanned
      ? 'Looking for projects...'
      : readyRepos.length > 0 && selectableRepos.length > 0
        ? `${readyRepos.length} configured, ${selectableRepos.length} available to set up.`
        : readyRepos.length > 0 && selectableRepos.length === 0
          ? `All ${readyRepos.length} project${readyRepos.length !== 1 ? 's are' : ' is'} configured. Add more below.`
          : `Found ${selectableRepos.length} project${selectableRepos.length !== 1 ? 's' : ''}. Select which to index.`;

  return (
    <div className="screen">
      <h2><FolderOpen size={17} /> Repositories</h2>
      <p className="subtitle">{subtitle}</p>

      <div className="screen-body">
        {error && (
          <div className="error-banner">
            <AlertCircle size={14} />
            <span>{error}</span>
          </div>
        )}

        {scanning && (
          <>
            <div className="scan-loading">
              <Loader2 size={15} className="spinning" />
              <span>Scanning directories...</span>
            </div>
            <div className="skeleton-list">
              {[0, 1, 2, 3].map(i => (
                <div key={i} className="skeleton-row">
                  <div className="skeleton-block skeleton-check" />
                  <div className="skeleton-block skeleton-icon" />
                  <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 6 }}>
                    <div className="skeleton-block skeleton-text" />
                    <div className="skeleton-block skeleton-meta" />
                  </div>
                </div>
              ))}
            </div>
          </>
        )}

        {/* Already configured repos — collapsed summary */}
        {readyRepos.length > 0 && !scanning && (
          <div className="ready-section">
            <button
              className="ready-header"
              onClick={() => setReadyExpanded(!readyExpanded)}
            >
              <CheckCircle2 size={14} className="ready-icon" />
              <span className="ready-label">
                {readyRepos.length} project{readyRepos.length !== 1 ? 's' : ''} ready
              </span>
              {readyExpanded
                ? <ChevronDown size={13} className="ready-chevron" />
                : <ChevronRight size={13} className="ready-chevron" />
              }
            </button>
            <div className={`collapse-container ${readyExpanded ? 'expanded' : ''}`}>
              <ul className="ready-list">
                {readyRepos.map(repo => (
                  <li key={repo.path} className="ready-item">
                    <FolderOpen size={13} className="ready-item-icon" />
                    <span className="ready-item-name">{repo.name}</span>
                    {repo.status_detail && (
                      <span className="ready-item-meta">
                        <Brain size={10} />
                        {repo.status_detail}
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          </div>
        )}

        {/* Selectable repos */}
        {!scanning && scanned && selectableRepos.length === 0 && readyRepos.length === 0 && !error && (
          <div className="empty-state">
            No projects found. Use "Add Directory" below to add one manually.
          </div>
        )}

        {selectableRepos.length > 0 && (
          <>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
              <button
                className="btn btn-secondary"
                onClick={selectAll}
                style={{ padding: '4px 12px', fontSize: '0.62rem' }}
              >
                {selectedRepos.length === selectableRepos.length ? 'Deselect All' : 'Select All'}
              </button>
              <span style={{ fontSize: '0.62rem', color: 'var(--text3)' }}>
                {selectedRepos.length} of {selectableRepos.length} selected
              </span>
            </div>
            <ul className="repo-list" key={`repos-${scanCount}`}>
              {selectableRepos.map((repo, idx) => {
                const isSelected = selectedRepos.some((r) => r.path === repo.path);
                return (
                  <li
                    key={repo.path}
                    className={`repo-item ${isSelected ? 'selected' : ''}`}
                    style={{ '--row-idx': idx } as React.CSSProperties}
                    onClick={() => toggleRepo(repo)}
                  >
                    <span className={`custom-check ${isSelected ? 'checked' : ''}`}>
                      <Check size={10} strokeWidth={3} />
                    </span>
                    <FolderOpen size={15} className="repo-icon" />
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div className="repo-name">
                        {repo.name}
                        {repo.status === 'stale' && (
                          <span className="status-badge stale">
                            <RefreshCw size={9} /> outdated
                          </span>
                        )}
                        {repo.status === 'needs_setup' && (
                          <span className="status-badge needs-setup">not indexed</span>
                        )}
                      </div>
                      {repo.status_detail && (
                        <div className={`repo-status-detail ${repo.status}`}>
                          {repo.status_detail}
                        </div>
                      )}
                      <div className="repo-path" title={repo.path}>{repo.path}</div>
                      <div className="repo-meta">
                        {repo.ecosystems.map((e) => (
                          <span key={e} className="ecosystem-tag">{e}</span>
                        ))}
                        {repo.file_count > 0 && (
                          <span className="file-count">
                            {repo.file_count.toLocaleString()} files
                          </span>
                        )}
                      </div>
                    </div>
                  </li>
                );
              })}
            </ul>
          </>
        )}

        <div style={{ display: 'flex', gap: 8, alignSelf: 'flex-start' }}>
          <button className="btn btn-secondary" onClick={addDirectory}>
            <FolderPlus size={13} /> Add Directory
          </button>
          {scanned && !scanning && (
            <button className="btn btn-secondary" onClick={scanDirs}>
              <Loader2 size={13} /> Rescan
            </button>
          )}
        </div>
      </div>

      <div className="btn-row">
        <button className="btn btn-secondary" onClick={onBack}>
          <ArrowLeft size={13} /> Back
        </button>
        <button
          className="btn btn-primary"
          onClick={onNext}
          disabled={selectedRepos.length === 0 && readyRepos.length === 0}
          autoFocus={!scanning}
        >
          {selectedRepos.length === 0 && readyRepos.length > 0
            ? <>Skip <ArrowRight size={13} /></>
            : <>Continue <ArrowRight size={13} /></>
          }
        </button>
      </div>
    </div>
  );
}
