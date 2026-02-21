import React from 'react';
import ReactDOM from 'react-dom/client';
import { ApiProvider } from '../shared/api';
import { SearchWindow } from './SearchWindow';
import '../styles/variables.css';
import './search-window.css';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ApiProvider value={{ tauri: true }}>
      <SearchWindow />
    </ApiProvider>
  </React.StrictMode>,
);
