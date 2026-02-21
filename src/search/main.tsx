import React from 'react';
import ReactDOM from 'react-dom/client';
import { ApiProvider } from '../shared/api';
import { SearchWindow } from './SearchWindow';
import '../styles/variables.css';
import './search-window.css';

const params = new URLSearchParams(window.location.search);
const port = params.get('port') || '8432';
const baseUrl = `http://127.0.0.1:${port}`;

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ApiProvider value={{ baseUrl }}>
      <SearchWindow />
    </ApiProvider>
  </React.StrictMode>,
);
