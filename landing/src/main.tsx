import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter, Route, Routes } from 'react-router-dom';
import { App } from './App.tsx';
import { PRIVACY_URL, TERMS_URL } from './constants/index.ts';
import './index.css';
import { Privacy } from './pages/Privacy.tsx';
import { Terms } from './pages/Terms.tsx';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<App />} />
        <Route path={TERMS_URL} element={<Terms />} />
        <Route path={PRIVACY_URL} element={<Privacy />} />
        <Route path="*" element={<App />} />
      </Routes>
    </BrowserRouter>
  </StrictMode>,
);
