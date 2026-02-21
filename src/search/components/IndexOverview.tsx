import { useState, useEffect } from 'react';
import { useIsTauri } from '../../shared/api';
import { getExtColor } from '../../colors';
import { FileIcon } from '../../icons';

interface RepoInfo {
  name: string;
  root: string;
  files: number;
  scanTime: number;
}

interface LangInfo {
  ext: string;
  count: number;
}

interface StatusData {
  repos: RepoInfo[];
  totalFiles: number;
  topLangs: LangInfo[];
}

export function IndexOverview() {
  const isTauri = useIsTauri();
  const [status, setStatus] = useState<StatusData | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    (async () => {
      try {
        let data: StatusData;
        if (isTauri) {
          const { invoke } = await import('@tauri-apps/api/core');
          data = await invoke('search_status');
        } else {
          const r = await fetch('/api/status');
          data = await r.json();
        }
        setStatus(data);
      } catch (e: any) {
        setError(e.message ?? String(e));
      }
    })();
  }, [isTauri]);

  if (error) {
    return (
      <div className="sw-overview">
        <div className="sw-overview-error">{error}</div>
      </div>
    );
  }

  if (!status) {
    return (
      <div className="sw-overview">
        <div className="sw-overview-loading">Scanning repos...</div>
      </div>
    );
  }

  const maxLangCount = status.topLangs[0]?.count ?? 1;

  return (
    <div className="sw-overview">
      <div className="sw-overview-section">
        <div className="sw-overview-heading">Indexed Repositories</div>
        {status.repos.map(repo => (
          <div key={repo.name} className="sw-overview-repo">
            <span className="sw-overview-repo-name">{repo.name}</span>
            <span className="sw-overview-repo-meta">
              {repo.files.toLocaleString()} files · {repo.scanTime}ms
            </span>
            <span className="sw-overview-repo-path">{repo.root}</span>
          </div>
        ))}
      </div>

      <div className="sw-overview-section">
        <div className="sw-overview-heading">
          Languages
          <span className="sw-overview-total">{status.totalFiles.toLocaleString()} files</span>
        </div>
        <div className="sw-overview-langs">
          {status.topLangs.map(lang => (
            <div key={lang.ext} className="sw-overview-lang">
              <div className="sw-overview-lang-label">
                <FileIcon ext={lang.ext} size={12} />
                <span>.{lang.ext}</span>
                <span className="sw-overview-lang-count">{lang.count.toLocaleString()}</span>
              </div>
              <div className="sw-overview-lang-bar-bg">
                <div
                  className="sw-overview-lang-bar"
                  style={{
                    width: `${(lang.count / maxLangCount) * 100}%`,
                    background: getExtColor(lang.ext),
                  }}
                />
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
