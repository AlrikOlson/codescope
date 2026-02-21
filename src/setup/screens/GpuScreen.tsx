import React, { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  Cpu, ArrowLeft, ArrowRight, Download, Check, X,
  AlertTriangle, Loader2, ExternalLink, Copy, Monitor,
} from 'lucide-react';

interface GpuInfo {
  gpu_detected: boolean;
  gpu_name: string;
  driver_version: string;
  cuda_installed: boolean;
  cuda_version: string;
  cuda_path: string;
  platform: string;
  can_auto_install: boolean;
  manual_install_cmd: string;
}

interface CudaInstallEvent {
  status: string;
  progress: number;
  message: string;
}

interface Props {
  onNext: () => void;
  onBack: () => void;
}

export function GpuScreen({ onNext, onBack }: Props) {
  const [gpu, setGpu] = useState<GpuInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [installState, setInstallState] = useState<CudaInstallEvent | null>(null);
  const [copied, setCopied] = useState(false);
  const copyTimeout = useRef<ReturnType<typeof setTimeout>>();

  // Detect GPU on mount
  useEffect(() => {
    invoke<GpuInfo>('detect_gpu')
      .then((info) => { setGpu(info); setLoading(false); })
      .catch((e) => { console.error('detect_gpu failed:', e); setLoading(false); });
  }, []);

  // Listen for install progress events
  useEffect(() => {
    const unlisten = listen<CudaInstallEvent>('cuda-install-progress', (event) => {
      setInstallState(event.payload);
      // Re-detect after completion
      if (event.payload.status === 'complete') {
        setTimeout(() => {
          invoke<GpuInfo>('detect_gpu').then(setGpu).catch(() => {});
        }, 1000);
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  const startInstall = () => {
    setInstallState({ status: 'downloading', progress: 0, message: 'Starting...' });
    invoke('install_cuda').catch((e) => {
      setInstallState({ status: 'failed', progress: 0, message: String(e) });
    });
  };

  const copyCommand = () => {
    if (gpu?.manual_install_cmd) {
      navigator.clipboard.writeText(gpu.manual_install_cmd);
      setCopied(true);
      if (copyTimeout.current) clearTimeout(copyTimeout.current);
      copyTimeout.current = setTimeout(() => setCopied(false), 2000);
    }
  };

  const redetect = () => {
    setLoading(true);
    invoke<GpuInfo>('detect_gpu')
      .then((info) => { setGpu(info); setLoading(false); })
      .catch(() => setLoading(false));
  };

  const installing = installState &&
    (installState.status === 'downloading' || installState.status === 'installing');

  return (
    <div className="screen">
      <h2><Monitor size={17} /> GPU Acceleration</h2>
      <p className="subtitle">
        CUDA-enabled GPUs dramatically speed up semantic embedding.
        CodeScope will auto-detect your hardware and can install the CUDA toolkit for you.
      </p>

      <div className="screen-body">
        {loading ? (
          <div className="gpu-detect-loading">
            <Loader2 size={16} className="spinning" />
            <span>Detecting hardware...</span>
          </div>
        ) : gpu ? (
          <>
            {/* GPU detection card */}
            <div className={`gpu-card ${gpu.gpu_detected ? 'detected' : 'none'}`}>
              <div className="gpu-card-icon">
                {gpu.gpu_detected
                  ? <Cpu size={18} style={{ color: 'var(--green)' }} />
                  : <Cpu size={18} style={{ color: 'var(--text3)' }} />}
              </div>
              <div className="gpu-card-body">
                <div className="gpu-card-title">
                  {gpu.gpu_detected ? gpu.gpu_name : 'No NVIDIA GPU Detected'}
                </div>
                {gpu.gpu_detected && gpu.driver_version && (
                  <div className="gpu-card-meta">Driver {gpu.driver_version}</div>
                )}
                {!gpu.gpu_detected && (
                  <div className="gpu-card-meta">
                    Semantic search will use CPU — still works, just slower.
                  </div>
                )}
              </div>
              {gpu.gpu_detected && (
                <div className="gpu-card-badge detected">
                  <Check size={10} /> Detected
                </div>
              )}
            </div>

            {/* CUDA toolkit status */}
            {gpu.gpu_detected && (
              <div className={`gpu-card ${gpu.cuda_installed ? 'detected' : 'missing'}`}>
                <div className="gpu-card-icon">
                  {gpu.cuda_installed
                    ? <Check size={18} style={{ color: 'var(--green)' }} />
                    : <AlertTriangle size={18} style={{ color: 'var(--yellow)' }} />}
                </div>
                <div className="gpu-card-body">
                  <div className="gpu-card-title">
                    {gpu.cuda_installed
                      ? `CUDA Toolkit ${gpu.cuda_version}`
                      : 'CUDA Toolkit Not Installed'}
                  </div>
                  <div className="gpu-card-meta">
                    {gpu.cuda_installed
                      ? gpu.cuda_path
                      : 'Required for GPU-accelerated embeddings'}
                  </div>
                </div>
                {gpu.cuda_installed && (
                  <div className="gpu-card-badge detected">
                    <Check size={10} /> Ready
                  </div>
                )}
              </div>
            )}

            {/* macOS message */}
            {gpu.platform === 'macos' && (
              <div className="gpu-info-banner">
                macOS uses the Accelerate framework — no CUDA needed.
              </div>
            )}

            {/* Auto-install button (Windows) */}
            {gpu.can_auto_install && !installState && (
              <div className="gpu-install-section">
                <button className="btn btn-accent gpu-install-btn" onClick={startInstall}>
                  <Download size={13} /> Install CUDA 12.6 Toolkit
                </button>
                <span className="gpu-install-note">
                  Downloads ~30 MB installer, then installs silently. Admin access required.
                </span>
              </div>
            )}

            {/* Install progress */}
            {installState && installState.status !== 'complete' && installState.status !== 'failed' && (
              <div className="gpu-install-progress">
                <div className="gpu-progress-header">
                  <Loader2 size={14} className="spinning" style={{ color: 'var(--accent)' }} />
                  <span>{installState.message}</span>
                </div>
                {installState.status === 'downloading' && (
                  <div className="semantic-bar-track">
                    <div
                      className="semantic-bar-fill"
                      style={{ width: `${Math.round(installState.progress * 100)}%` }}
                    />
                  </div>
                )}
                {installState.status === 'installing' && (
                  <div className="semantic-bar-track">
                    <div
                      className={`semantic-bar-fill${installState.progress <= 0 ? ' indeterminate' : ''}`}
                      style={installState.progress > 0 ? { width: `${Math.round(installState.progress * 100)}%`, transition: 'width 0.6s ease' } : undefined}
                    />
                  </div>
                )}
              </div>
            )}

            {/* Install complete */}
            {installState?.status === 'complete' && (
              <div className="gpu-result-banner success">
                <Check size={14} />
                <span>{installState.message}</span>
              </div>
            )}

            {/* Install failed */}
            {installState?.status === 'failed' && (
              <div className="gpu-result-banner error">
                <X size={14} />
                <span>{installState.message}</span>
              </div>
            )}

            {/* Manual install command (Linux) */}
            {gpu.manual_install_cmd && !installState && (
              <div className="gpu-manual-section">
                <div className="gpu-manual-label">
                  Run this in your terminal to install CUDA:
                </div>
                <div className="gpu-manual-cmd">
                  <pre>{gpu.manual_install_cmd}</pre>
                  <button
                    className={`gpu-copy-btn ${copied ? 'copied' : ''}`}
                    onClick={copyCommand}
                    title="Copy to clipboard"
                  >
                    {copied ? <Check size={12} /> : <Copy size={12} />}
                  </button>
                </div>
                <button className="btn btn-secondary gpu-redetect-btn" onClick={redetect}>
                  Re-detect after installing
                </button>
              </div>
            )}

            {/* Link to NVIDIA download page */}
            {gpu.gpu_detected && !gpu.cuda_installed && !installing && (
              <a
                className="gpu-download-link"
                href="https://developer.nvidia.com/cuda-downloads"
                target="_blank"
                rel="noopener noreferrer"
              >
                <ExternalLink size={11} /> Or download manually from NVIDIA
              </a>
            )}
          </>
        ) : (
          <div className="gpu-detect-loading">
            <AlertTriangle size={16} style={{ color: 'var(--yellow)' }} />
            <span>Could not detect GPU hardware</span>
          </div>
        )}
      </div>

      <div className="btn-row">
        <button className="btn btn-secondary" onClick={onBack} disabled={!!installing}>
          <ArrowLeft size={13} /> Back
        </button>
        <button className="btn btn-primary" onClick={onNext} autoFocus disabled={!!installing}>
          Continue <ArrowRight size={13} />
        </button>
      </div>
    </div>
  );
}
