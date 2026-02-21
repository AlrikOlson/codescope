import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

interface RepoInfo {
  path: string;
  name: string;
  ecosystems: string[];
  workspace_info: string | null;
  file_count: number;
}

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
      // Add to discovered if not already there
      if (!discoveredRepos.some((r) => r.path === repo.path)) {
        setDiscoveredRepos([...discoveredRepos, repo]);
      }
      // Auto-select
      if (!selectedRepos.some((r) => r.path === repo.path)) {
        onSelectedReposChange([...selectedRepos, repo]);
      }
    }
  };

  return (
    <div className="screen">
      <h2>Select Repositories</h2>
      <p className="subtitle">
        We found {discoveredRepos.length} projects. Select which ones to index.
      </p>

      {scanning && <p style={{ color: '#888' }}>Scanning for projects...</p>}

      <ul className="repo-list">
        {discoveredRepos.map((repo) => {
          const isSelected = selectedRepos.some((r) => r.path === repo.path);
          return (
            <li
              key={repo.path}
              className={`repo-item ${isSelected ? 'selected' : ''}`}
              onClick={() => toggleRepo(repo)}
            >
              <input
                type="checkbox"
                checked={isSelected}
                onChange={() => toggleRepo(repo)}
              />
              <div style={{ flex: 1 }}>
                <div className="repo-name">{repo.name}</div>
                <div className="repo-meta" style={{ marginBottom: '0.25rem' }}>
                  <span style={{ color: '#555', fontSize: '0.75rem' }}>{repo.path}</span>
                </div>
                <div className="repo-meta">
                  {repo.ecosystems.map((e) => (
                    <span key={e} className="ecosystem-tag">{e}</span>
                  ))}
                  {repo.workspace_info && (
                    <span style={{ marginLeft: '0.5rem' }}>{repo.workspace_info}</span>
                  )}
                  {repo.file_count > 0 && (
                    <span style={{ marginLeft: '0.5rem' }}>
                      {repo.file_count.toLocaleString()} files
                    </span>
                  )}
                </div>
              </div>
            </li>
          );
        })}
      </ul>

      <button className="btn btn-secondary" onClick={addDirectory} style={{ alignSelf: 'flex-start' }}>
        + Add Directory
      </button>

      <div className="btn-row">
        <button className="btn btn-secondary" onClick={onBack}>Back</button>
        <button
          className="btn btn-primary"
          onClick={onNext}
          disabled={selectedRepos.length === 0}
        >
          Next ({selectedRepos.length} selected)
        </button>
      </div>
    </div>
  );
}
