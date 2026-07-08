import '@testing-library/jest-dom';
import type { Mock } from 'vitest';

// jsdom ships neither IntersectionObserver nor ResizeObserver; DiffView's
// file-level virtualization needs both. Stub them: IO reports every observed
// target as immediately intersecting so lazily-mounted diff content renders in
// tests just like it does on first paint in the app.
class IntersectionObserverStub {
  private cb: IntersectionObserverCallback;
  constructor(cb: IntersectionObserverCallback) {
    this.cb = cb;
  }
  observe(el: Element) {
    this.cb([{ isIntersecting: true, target: el } as IntersectionObserverEntry], this as unknown as IntersectionObserver);
  }
  unobserve() {}
  disconnect() {}
  takeRecords() {
    return [];
  }
}
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
(globalThis as unknown as { IntersectionObserver: unknown }).IntersectionObserver = IntersectionObserverStub;
(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = ResizeObserverStub;

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
