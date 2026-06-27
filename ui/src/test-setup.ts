import '@testing-library/jest-dom';
import type { Mock } from 'vitest';

// Polyfill localStorage for jsdom
Object.defineProperty(window, 'localStorage', {
  value: {
    getItem: (() => null) as Mock,
    setItem: (() => {}) as Mock,
    removeItem: (() => {}) as Mock,
    clear: (() => {}) as Mock,
    get length() {
      return 0;
    },
    get key() {
      return null;
    },
  },
  writable: true,
});
