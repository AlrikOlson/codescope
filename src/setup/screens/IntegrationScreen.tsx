import React, { useState, useEffect, useCallback, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  Puzzle, Terminal, Globe, Check, ArrowLeft, ArrowRight,
  Loader2, Wrench, RefreshCw,
} from 'lucide-react';
import type { RepoInfo } from '../SetupWizard';

interface Props {
  selectedRepos: RepoInfo[];
  onNext: () => void;
  onBack: () => void;
}

interface McpStatus {
  configured: number;
  total: number;
  missing: string[];
}

type RowStatus = 'checking' | 'ok' | 'action_needed' | 'error';

function detectCompletionsShell(): string {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes('windows')) return 'powershell';
  if (ua.includes('mac')) return 'zsh';
  return 'bash';
}

export function IntegrationScreen({ selectedRepos, onNext, onBack }: Props) {
  // --- PATH state ---
  const [pathStatus, setPathStatus] = useState<RowStatus>('checking');
  const [pathMsg, setPathMsg] = useState('');
  const [pathBusy, setPathBusy] = useState(false);

  // --- MCP state ---
  const [mcpStatus, setMcpStatus] = useState<RowStatus>('checking');
  const [mcpMsg, setMcpMsg] = useState('');
  const [mcpBusy, setMcpBusy] = useState(false);
  const mcpMissing = useRef<string[]>([]);

  // --- Completions state ---
  const [compStatus, setCompStatus] = useState<RowStatus>('checking');
  const [compMsg, setCompMsg] = useState('');
  const [compBusy, setCompBusy] = useState(false);

  const completionsShell = detectCompletionsShell();
  const isOnPath = pathStatus === 'ok';

  // --- Check all on mount ---
  const checkAll = useCallback(async () => {
    // PATH
    setPathStatus('checking');
    try {
      const ok = await invoke<boolean>('check_on_path');
      setPathStatus(ok ? 'ok' : 'action_needed');
      setPathMsg(ok ? 'codescope is accessible from any terminal.' : 'codescope is not on your PATH.');
    } catch {
      setPathStatus('error');
      setPathMsg('Could not check PATH status.');
    }

    // MCP
    setMcpStatus('checking');
    try {
      const paths = selectedRepos.map((r) => r.path);
      if (paths.length === 0) {
        setMcpStatus('ok');
        setMcpMsg('Will be configured when projects are initialized.');
        mcpMissing.current = [];
      } else {
        const status = await invoke<McpStatus>('check_mcp_status', { paths });
        mcpMissing.current = status.missing;
        if (status.configured === status.total) {
          setMcpStatus('ok');
          setMcpMsg(`All ${status.total} project${status.total === 1 ? '' : 's'} configured.`);
        } else {
          setMcpStatus('action_needed');
          setMcpMsg(`${status.configured}/${status.total} projects have .mcp.json configured.`);
        }
      }
    } catch {
      setMcpStatus('error');
      setMcpMsg('Could not check MCP status.');
    }

    // Completions
    setCompStatus('checking');
    try {
      const ok = await invoke<boolean>('check_completions');
      setCompStatus(ok ? 'ok' : 'action_needed');
      setCompMsg(ok
        ? `${completionsShell} completions installed.`
        : `Tab completions for ${completionsShell} not installed.`);
    } catch {
      setCompStatus('error');
      setCompMsg('Could not check completions status.');
    }
  }, [selectedRepos, completionsShell]);

  useEffect(() => { checkAll(); }, [checkAll]);

  // --- Actions ---
  const handleFixPath = useCallback(async () => {
    setPathBusy(true);
    try {
      const msg = await invoke<string>('fix_path');
      setPathMsg(msg);
      const ok = await invoke<boolean>('check_on_path');
      setPathStatus(ok ? 'ok' : 'action_needed');
    } catch (e) {
      setPathMsg(String(e));
      setPathStatus('error');
    } finally {
      setPathBusy(false);
    }
  }, []);

  const handleConfigureMcp = useCallback(async () => {
    setMcpBusy(true);
    try {
      const msg = await invoke<string>('configure_mcp', { paths: mcpMissing.current });
      setMcpMsg(msg);
      // Re-check
      const paths = selectedRepos.map((r) => r.path);
      const status = await invoke<McpStatus>('check_mcp_status', { paths });
      mcpMissing.current = status.missing;
      setMcpStatus(status.configured === status.total ? 'ok' : 'action_needed');
    } catch (e) {
      setMcpMsg(String(e));
      setMcpStatus('error');
    } finally {
      setMcpBusy(false);
    }
  }, [selectedRepos]);

  const handleInstallCompletions = useCallback(async () => {
    setCompBusy(true);
    try {
      const msg = await invoke<string>('install_completions');
      setCompMsg(msg);
      setCompStatus('ok');
    } catch (e) {
      setCompMsg(String(e));
      setCompStatus('error');
    } finally {
      setCompBusy(false);
    }
  }, []);

  // --- Render helpers ---
  const statusBadge = (status: RowStatus, busy: boolean, action: () => void, actionLabel: string, disabled?: boolean) => {
    if (status === 'checking') {
      return (
        <span className="toggle-status" style={{ color: 'var(--text3)' }}>
          <Loader2 size={12} className="spinning" /> Checking
        </span>
      );
    }
    if (status === 'ok') {
      return (
        <span className="toggle-status status-pass">
          <Check size={13} /> OK
        </span>
      );
    }
    if (status === 'error') {
      return (
        <button
          className="btn btn-sm btn-secondary"
          onClick={action}
          disabled={busy}
        >
          {busy
            ? <><Loader2 size={12} className="spinning" /> Retrying...</>
            : <><RefreshCw size={12} /> Retry</>
          }
        </button>
      );
    }
    // action_needed
    return (
      <button
        className="btn btn-sm btn-accent"
        onClick={action}
        disabled={busy || disabled}
        title={disabled ? 'Fix PATH first' : undefined}
      >
        {busy
          ? <><Loader2 size={12} className="spinning" /> Working...</>
          : <><Wrench size={12} /> {actionLabel}</>
        }
      </button>
    );
  };

  const anyBusy = pathBusy || mcpBusy || compBusy;

  return (
    <div className="screen">
      <h2><Puzzle size={17} /> Integrations</h2>
      <p className="subtitle">
        CodeScope integrates with your shell, editor, and Claude Code.
        Each item is checked automatically — press a button to fix what's needed.
      </p>

      <div className="screen-body">
        {/* PATH — foundational, others depend on it */}
        <div className="toggle-row" style={{ '--row-idx': 0 } as React.CSSProperties}>
          <div className="toggle-row-left">
            <Globe size={16} className="toggle-row-icon" />
            <div>
              <div className="toggle-label">PATH</div>
              <div className="toggle-description">{pathMsg || 'Checking...'}</div>
            </div>
          </div>
          {statusBadge(pathStatus, pathBusy, handleFixPath, 'Fix')}
        </div>

        {/* MCP */}
        <div className="toggle-row" style={{ '--row-idx': 1 } as React.CSSProperties}>
          <div className="toggle-row-left">
            <Puzzle size={16} className="toggle-row-icon" />
            <div>
              <div className="toggle-label">Claude Code (MCP)</div>
              <div className="toggle-description">{mcpMsg || 'Checking...'}</div>
            </div>
          </div>
          {statusBadge(mcpStatus, mcpBusy, handleConfigureMcp, 'Configure')}
        </div>

        {/* Shell Completions */}
        <div className="toggle-row" style={{ '--row-idx': 2 } as React.CSSProperties}>
          <div className="toggle-row-left">
            <Terminal size={16} className="toggle-row-icon" />
            <div>
              <div className="toggle-label">Shell Completions</div>
              <div className="toggle-description">
                {compStatus === 'ok'
                  ? compMsg
                  : !isOnPath
                    ? 'Requires codescope on PATH first.'
                    : compMsg || 'Checking...'
                }
              </div>
            </div>
          </div>
          {statusBadge(compStatus, compBusy, handleInstallCompletions, 'Install', !isOnPath)}
        </div>
      </div>

      <div className="btn-row">
        <button className="btn btn-secondary" onClick={onBack} disabled={anyBusy}>
          <ArrowLeft size={13} /> Back
        </button>
        <button className="btn btn-primary" onClick={onNext} autoFocus disabled={anyBusy}>
          Continue <ArrowRight size={13} />
        </button>
      </div>
    </div>
  );
}
