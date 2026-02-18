import ReactDOM from 'react-dom/client';
import App from './App';
import './styles/variables.css';
import './styles/crt.css';

// Prevent FOUC: apply saved theme before first render
const savedTheme = localStorage.getItem('codescope-theme');
if (savedTheme === 'light' || savedTheme === 'dark') {
  document.documentElement.style.colorScheme = savedTheme;
}

ReactDOM.createRoot(document.getElementById('root')!).render(<App />);
