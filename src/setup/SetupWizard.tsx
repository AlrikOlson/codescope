import React, { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { X, Check } from 'lucide-react';
import { WelcomeScreen } from './screens/WelcomeScreen';
import { RepoPickerScreen } from './screens/RepoPickerScreen';
import { SemanticScreen } from './screens/SemanticScreen';
import { IntegrationScreen } from './screens/IntegrationScreen';
import { DoctorScreen } from './screens/DoctorScreen';
import { DoneScreen } from './screens/DoneScreen';
import './setup.css';

type Screen = 'welcome' | 'repos' | 'semantic' | 'integration' | 'doctor' | 'done';

export interface RepoInfo {
  path: string;
  name: string;
  ecosystems: string[];
  workspace_info: string | null;
  file_count: number;
  /** "ready" | "stale" | "needs_setup" | "new" */
  status: 'ready' | 'stale' | 'needs_setup' | 'new';
  /** Human-readable explanation of the status */
  status_detail: string;
  semantic_chunks: number;
  semantic_model: string;
}

interface GlobalConfig {
  repos: { name: string; path: string }[];
  version: string;
  has_semantic: boolean;
}

const STEPS: { id: Screen; label: string }[] = [
  { id: 'welcome', label: 'Welcome' },
  { id: 'repos', label: 'Repositories' },
  { id: 'semantic', label: 'Semantic' },
  { id: 'integration', label: 'Integrations' },
  { id: 'doctor', label: 'Initialize' },
  { id: 'done', label: 'Done' },
];

export function SetupWizard() {
  const [screen, setScreen] = useState<Screen>('welcome');
  const [direction, setDirection] = useState<'forward' | 'back'>('forward');
  const [version, setVersion] = useState('');
  const [config, setConfig] = useState<GlobalConfig | null>(null);
  const [selectedRepos, setSelectedRepos] = useState<RepoInfo[]>([]);
  const [enableSemantic, setEnableSemantic] = useState(true);
  const [semanticModel, setSemanticModel] = useState('standard');

  useEffect(() => {
    invoke<string>('get_version')
      .then(setVersion)
      .catch((e) => console.error('get_version failed:', e));
    invoke<GlobalConfig>('get_config')
      .then(setConfig)
      .catch((e) => console.error('get_config failed:', e));
  }, []);

  const currentIndex = STEPS.findIndex((s) => s.id === screen);
  const progress = currentIndex / (STEPS.length - 1);

  const next = useCallback(() => {
    const idx = STEPS.findIndex((s) => s.id === screen);
    if (idx < STEPS.length - 1) {
      setDirection('forward');
      setScreen(STEPS[idx + 1].id);
    }
  }, [screen]);

  const back = useCallback(() => {
    const idx = STEPS.findIndex((s) => s.id === screen);
    if (idx > 0) {
      setDirection('back');
      setScreen(STEPS[idx - 1].id);
    }
  }, [screen]);

  const goToStep = useCallback((targetIndex: number) => {
    setDirection('back');
    setScreen(STEPS[targetIndex].id);
  }, []);

  // Global keyboard navigation: Escape to go back
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
      if (e.key === 'Escape' && currentIndex > 0) {
        e.preventDefault();
        back();
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [currentIndex, back]);

  return (
    <div className="setup-wizard">
      {/* Custom titlebar */}
      <div className="titlebar" data-tauri-drag-region>
        <span className="titlebar-label" data-tauri-drag-region>CodeScope Setup</span>
        <button
          className="titlebar-close"
          onClick={() => getCurrentWindow().close()}
          aria-label="Close"
        >
          <X size={13} />
        </button>
      </div>

      <div className="setup-body">
        {/* Step rail */}
        <nav className="step-rail">
          {STEPS.map((step, i) => {
            const state = i < currentIndex ? 'completed' : i === currentIndex ? 'current' : 'future';
            return (
              <React.Fragment key={step.id}>
                <div
                  className={`step-item ${state}`}
                  {...(state === 'completed' ? {
                    role: 'button',
                    tabIndex: 0,
                    onClick: () => goToStep(i),
                    onKeyDown: (e: React.KeyboardEvent) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        goToStep(i);
                      }
                    },
                  } : {})}
                >
                  <div className="step-circle">
                    {state === 'completed' ? <Check size={12} strokeWidth={3} /> : i + 1}
                  </div>
                  <span className="step-label">{step.label}</span>
                </div>
                {i < STEPS.length - 1 && (
                  <div className={`step-connector ${i < currentIndex ? 'done' : ''}`} />
                )}
              </React.Fragment>
            );
          })}
        </nav>

        {/* Main content */}
        <div className="setup-main">
          {/* Progress bar */}
          <div className="wizard-progress">
            <div
              className="wizard-progress-fill"
              style={{ width: `${progress * 100}%` }}
            />
          </div>

          <div
            className="setup-content"
            key={screen}
            data-direction={direction}
          >
            {screen === 'welcome' && (
              <WelcomeScreen version={version} onNext={next} />
            )}
            {screen === 'repos' && (
              <RepoPickerScreen
                selectedRepos={selectedRepos}
                onSelectedReposChange={setSelectedRepos}
                registeredPaths={config?.repos.map(r => r.path) ?? []}
                onNext={next}
                onBack={back}
              />
            )}
            {screen === 'semantic' && (
              <SemanticScreen
                enabled={enableSemantic}
                onEnabledChange={setEnableSemantic}
                selectedModel={semanticModel}
                onModelChange={setSemanticModel}
                hasSemantic={config?.has_semantic ?? false}
                onNext={next}
                onBack={back}
              />
            )}
            {screen === 'integration' && (
              <IntegrationScreen onNext={next} onBack={back} />
            )}
            {screen === 'doctor' && (
              <DoctorScreen
                repos={selectedRepos}
                semantic={enableSemantic}
                semanticModel={semanticModel}
                onNext={next}
                onBack={back}
              />
            )}
            {screen === 'done' && (
              <DoneScreen
                repoCount={selectedRepos.length}
                semantic={enableSemantic}
                semanticModel={semanticModel}
              />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
