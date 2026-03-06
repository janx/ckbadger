import { StrictMode } from 'react';
import ReactDOM from 'react-dom/client';
import { AppRoot } from '@/src/app/root';
import '@/app/globals.css';

const rootElement = document.getElementById('root');

if (!rootElement) {
  throw new Error('Missing #root element for SPA mount');
}

ReactDOM.createRoot(rootElement).render(
  <StrictMode>
    <AppRoot />
  </StrictMode>
);
