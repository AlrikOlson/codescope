import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { X } from 'lucide-react';
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

export function SetupWizard() {
  const [screen, setScreen] = useState<Screen>('welcome');
  const [version, setVersion] = useState('');
  const [config, setConfig] = useState<GlobalConfig | null>(null);
  const [selectedRepos, setSelectedRepos] = useState<RepoInfo[]>([]);
  const [enableSemantic, setEnableSemantic] = useState(true);

  useEffect(() => {
    invoke<string>('get_version').then(setVersion);
    invoke<GlobalConfig>('get_config').then(setConfig);
  }, []);

  const screens: Screen[] = ['welcome', 'repos', 'semantic', 'integration', 'doctor', 'done'];
  const currentIndex = screens.indexOf(screen);

  const next = () => {
    if (currentIndex < screens.length - 1) {
      setScreen(screens[currentIndex + 1]);
    }
  };

  const back = () => {
    if (currentIndex > 0) {
      setScreen(screens[currentIndex - 1]);
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
          <X size={14} />
        </button>
      </div>

      <div className="setup-content">
        {/* Progress bar */}
        <div className="progress-bar">
          {screens.map((s, i) => (
            <div
              key={s}
              className={`progress-dot ${i <= currentIndex ? 'active' : ''} ${i === currentIndex ? 'current' : ''}`}
            />
          ))}
        </div>

        {/* Screen content */}
        <div className="screen-content">
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
  );
}
