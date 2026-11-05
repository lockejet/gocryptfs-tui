// Three-state theme controller: auto (follow prefers-color-scheme) -> light -> dark.
//
// "auto" is applied as data-theme="" so app.css can fall back to the
// prefers-color-scheme media query; explicit light/dark set data-theme
// to that value. The choice is persisted in localStorage["gfs-theme"].

import { writable, get } from 'svelte/store';

export const THEME_KEY = 'gfs-theme';
export const MODES = ['auto', 'light', 'dark'];

const META_COLORS = { dark: '#0e1116', light: '#f6f8fb' };

function media() {
  if (typeof window === 'undefined' || !window.matchMedia) return null;
  return window.matchMedia('(prefers-color-scheme: dark)');
}

function storedMode() {
  try {
    const value = localStorage.getItem(THEME_KEY);
    if (MODES.includes(value)) return value;
  } catch (_) {
    /* storage unavailable (private mode / disabled) */
  }
  return 'auto';
}

export const themeMode = writable(storedMode());

export function resolveTheme(mode) {
  if (mode === 'light' || mode === 'dark') return mode;
  const mq = media();
  return mq && mq.matches ? 'dark' : 'light';
}

function apply() {
  if (typeof document === 'undefined') return;
  const mode = get(themeMode);
  const resolved = resolveTheme(mode);
  const root = document.documentElement;
  root.dataset.theme = mode === 'auto' ? '' : mode;
  root.style.colorScheme = resolved;
  const meta = document.querySelector('meta[name="theme-color"]');
  if (meta) meta.setAttribute('content', META_COLORS[resolved] || META_COLORS.dark);
}

export function cycleTheme() {
  const current = get(themeMode);
  const next = MODES[(MODES.indexOf(current) + 1) % MODES.length];
  themeMode.set(next);
  try {
    localStorage.setItem(THEME_KEY, next);
  } catch (_) {
    /* ignore */
  }
}

export function themeLabel(mode) {
  return mode === 'light' ? 'Light' : mode === 'dark' ? 'Dark' : 'Auto';
}

export function themeIcon(mode) {
  return mode === 'light' ? '☀' : mode === 'dark' ? '☾' : '◐';
}

// Subscribes to the store, applies immediately and re-applies whenever the
// OS colour scheme changes while in auto mode. Returns an unsubscribe fn.
export function initTheme() {
  const unsubscribe = themeMode.subscribe(() => apply());
  const mq = media();
  const onChange = () => {
    if (get(themeMode) === 'auto') apply();
  };
  if (mq) {
    if (mq.addEventListener) mq.addEventListener('change', onChange);
    else if (mq.addListener) mq.addListener(onChange);
  }
  return () => {
    unsubscribe();
    if (mq) {
      if (mq.removeEventListener) mq.removeEventListener('change', onChange);
      else if (mq.removeListener) mq.removeListener(onChange);
    }
  };
}
