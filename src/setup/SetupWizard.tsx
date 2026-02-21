import React, { useState, useEffect } from 'react';
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
  const [version, setVersion] = useState('');
  const [config, setConfig] = useState<GlobalConfig | null>(null);
  const [selectedRepos, setSelectedRepos] = useState<RepoInfo[]>([]);
  const [enableSemantic, setEnableSemantic] = useState(true);

  useEffect(() => {
    invoke<string>('get_version')
      .then(setVersion)
      .catch((e) => console.error('get_version failed:', e));
    invoke<GlobalConfig>('get_config')
      .then(setConfig)
      .catch((e) => console.error('get_config failed:', e));
  }, []);

  const currentIndex = STEPS.findIndex((s) => s.id === screen);

  const next = () => {
    if (currentIndex < STEPS.length - 1) {
      setScreen(STEPS[currentIndex + 1].id);
    }
  };

  const back = () => {
    if (currentIndex > 0) {
      setScreen(STEPS[currentIndex - 1].id);
    }
  };

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
                <div className={`step-item ${state}`}>
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
          <div className="setup-content" key={screen}>
            {screen === 'welcome' && (
              <WelcomeScreen version={version} onNext={next} />
            )}
            {screen === 'repos' && (
              <RepoPickerScreen
                selectedRepos={selectedRepos}
                onSelectedReposChange={setSelectedRepos}
                onNext={next}
                onBack={back}
              />
            )}
            {screen === 'semantic' && (
              <SemanticScreen
                enabled={enableSemantic}
                onEnabledChange={setEnableSemantic}
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
                onNext={next}
                onBack={back}
              />
            )}
            {screen === 'done' && (
              <DoneScreen
                repoCount={selectedRepos.length}
                semantic={enableSemantic}
              />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
