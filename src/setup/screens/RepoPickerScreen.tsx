import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { FolderOpen, FolderPlus, ArrowLeft, ArrowRight, Loader2, AlertCircle } from 'lucide-react';
import type { RepoInfo } from '../SetupWizard';

interface Props {
  selectedRepos: RepoInfo[];
  onSelectedReposChange: (repos: RepoInfo[]) => void;
  onNext: () => void;
  onBack: () => void;
}

export function RepoPickerScreen({ selectedRepos, onSelectedReposChange, onNext, onBack }: Props) {
  const [discoveredRepos, setDiscoveredRepos] = useState<RepoInfo[]>([]);
  const [scanning, setScanning] = useState(false);
  const [scanned, setScanned] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const scanDirs = async () => {
    setScanning(true);
    setError(null);
    try {
      const dirs = await invoke<string[]>('get_scan_dirs');
      const repos = await invoke<RepoInfo[]>('scan_for_repos', { dirs });
      setDiscoveredRepos(repos);
      setScanned(true);
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
    if (selectedRepos.length === discoveredRepos.length) {
      onSelectedReposChange([]);
    } else {
      onSelectedReposChange([...discoveredRepos]);
    }
  };

  const addDirectory = async () => {
    try {
      const selected = await open({ directory: true, multiple: false });
      if (selected) {
        const repo = await invoke<RepoInfo>('detect_project', { path: selected });
        if (!discoveredRepos.some((r) => r.path === repo.path)) {
          setDiscoveredRepos([...discoveredRepos, repo]);
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

  return (
    <div className="screen">
      <h2><FolderOpen size={17} /> Repositories</h2>
      <p className="subtitle">
        {scanning
          ? 'Scanning your filesystem for projects...'
          : scanned
            ? `Found ${discoveredRepos.length} project${discoveredRepos.length !== 1 ? 's' : ''}. Select which to index.`
            : 'Looking for projects...'}
      </p>

      {error && (
        <div className="error-banner">
          <AlertCircle size={14} />
          <span>{error}</span>
        </div>
      )}

      {scanning && (
        <div className="scan-loading">
          <Loader2 size={15} className="spinning" />
          <span>Scanning directories...</span>
        </div>
      )}

      {!scanning && scanned && discoveredRepos.length === 0 && !error && (
        <div className="empty-state">
          No projects found. Use "Add Directory" below to add one manually.
        </div>
      )}

      {discoveredRepos.length > 0 && (
        <>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
            <button
              className="btn btn-secondary"
              onClick={selectAll}
              style={{ padding: '4px 12px', fontSize: '0.65rem' }}
            >
              {selectedRepos.length === discoveredRepos.length ? 'Deselect All' : 'Select All'}
            </button>
            <span style={{ fontSize: '0.65rem', color: 'var(--text3)' }}>
              {selectedRepos.length} of {discoveredRepos.length} selected
            </span>
          </div>
          <ul className="repo-list">
            {discoveredRepos.map((repo, idx) => {
              const isSelected = selectedRepos.some((r) => r.path === repo.path);
              return (
                <li
                  key={repo.path}
                  className={`repo-item ${isSelected ? 'selected' : ''}`}
                  style={{ '--row-idx': idx } as React.CSSProperties}
                  onClick={() => toggleRepo(repo)}
                >
                  <input
                    type="checkbox"
                    checked={isSelected}
                    onChange={() => toggleRepo(repo)}
                    onClick={(e) => e.stopPropagation()}
                  />
                  <FolderOpen size={15} className="repo-icon" />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div className="repo-name">{repo.name}</div>
                    <div className="repo-path">{repo.path}</div>
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

      <div className="btn-row">
        <button className="btn btn-secondary" onClick={onBack}>
          <ArrowLeft size={13} /> Back
        </button>
        <button
          className="btn btn-primary"
          onClick={onNext}
          disabled={selectedRepos.length === 0}
        >
          Continue <ArrowRight size={13} />
        </button>
      </div>
    </div>
  );
}
