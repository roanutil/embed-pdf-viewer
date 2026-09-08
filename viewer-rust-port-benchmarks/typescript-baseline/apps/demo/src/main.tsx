import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { markBoot } from './marks';
import './styles.css';

markBoot();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
