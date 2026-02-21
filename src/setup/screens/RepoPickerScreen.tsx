import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { FolderOpen, FolderPlus, ArrowLeft, ArrowRight, Loader2 } from 'lucide-react';
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

  const scanDirs = async () => {
    setScanning(true);
    try {
      const dirs = await invoke<string[]>('get_scan_dirs');
      const repos = await invoke<RepoInfo[]>('scan_for_repos', { dirs });
      setDiscoveredRepos(repos);
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

  const addDirectory = async () => {
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
  };

  return (
    <div className="screen">
      <h2><FolderOpen size={18} /> Select Repositories</h2>
      <p className="subtitle">
        {scanning
          ? 'Scanning for projects...'
          : scanned
            ? `Found ${discoveredRepos.length} project${discoveredRepos.length !== 1 ? 's' : ''}. Select which to index.`
            : 'Looking for projects...'}
      </p>

      {scanning && (
        <div className="scan-loading">
          <Loader2 size={16} className="spinning" />
          <span>Scanning directories...</span>
        </div>
      )}

      {!scanning && scanned && discoveredRepos.length === 0 && (
        <div className="empty-state">
          No projects found. Use "Add Directory" to add one manually.
        </div>
      )}

      {discoveredRepos.length > 0 && (
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
                />
                <FolderOpen size={16} className="repo-icon" />
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
      )}

      <button className="btn btn-secondary" onClick={addDirectory} style={{ alignSelf: 'flex-start' }}>
        <FolderPlus size={14} /> Add Directory
      </button>

      <div className="btn-row">
        <button className="btn btn-secondary" onClick={onBack}>
          <ArrowLeft size={14} /> Back
        </button>
        <button
          className="btn btn-primary"
          onClick={onNext}
          disabled={selectedRepos.length === 0}
        >
          Next ({selectedRepos.length}) <ArrowRight size={14} />
        </button>
      </div>
    </div>
  );
}
