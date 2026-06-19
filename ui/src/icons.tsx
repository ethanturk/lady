import type { Component, JSX } from "solid-js";

/**
 * Small stroke-icon set for the Fork-style UI (design/README.md uses inline SVG,
 * stroke=currentColor, ~16px). Each icon inherits color from its parent's
 * `color`, so toolbar/sidebar tokens (--tx*, --accent) drive them for free.
 */
type IconProps = { size?: number; style?: JSX.CSSProperties };

const svg = (path: JSX.Element, size = 16, style?: JSX.CSSProperties): JSX.Element => (
  <svg
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.6"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
    style={style}
  >
    {path}
  </svg>
);

export const IconFetch: Component<IconProps> = (p) =>
  svg(
    <>
      <path d="M21 12a9 9 0 1 1-3-6.7" />
      <path d="M21 4v4h-4" />
    </>,
    p.size,
    p.style,
  );

export const IconPull: Component<IconProps> = (p) =>
  svg(
    <>
      <path d="M12 3v14" />
      <path d="M6 11l6 6 6-6" />
      <path d="M5 21h14" />
    </>,
    p.size,
    p.style,
  );

export const IconPush: Component<IconProps> = (p) =>
  svg(
    <>
      <path d="M12 21V7" />
      <path d="M6 13l6-6 6 6" />
      <path d="M5 3h14" />
    </>,
    p.size,
    p.style,
  );

export const IconStash: Component<IconProps> = (p) =>
  svg(
    <>
      <path d="M3 8l1.5 11a2 2 0 0 0 2 1.8h11a2 2 0 0 0 2-1.8L21 8" />
      <path d="M2 4h20v4H2z" />
      <path d="M10 12h4" />
    </>,
    p.size,
    p.style,
  );

export const IconBranch: Component<IconProps> = (p) =>
  svg(
    <>
      <circle cx="6" cy="6" r="2.5" />
      <circle cx="6" cy="18" r="2.5" />
      <circle cx="18" cy="7" r="2.5" />
      <path d="M6 8.5v7" />
      <path d="M18 9.5c0 4-6 3-6 7" />
    </>,
    p.size,
    p.style,
  );

export const IconCommits: Component<IconProps> = (p) =>
  svg(
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M3 12h6" />
      <path d="M15 12h6" />
    </>,
    p.size,
    p.style,
  );

export const IconChanges: Component<IconProps> = (p) =>
  svg(
    <>
      <path d="M4 6h16" />
      <path d="M4 12h16" />
      <path d="M4 18h10" />
    </>,
    p.size,
    p.style,
  );

export const IconMore: Component<IconProps> = (p) =>
  svg(
    <>
      <circle cx="5" cy="12" r="1.4" />
      <circle cx="12" cy="12" r="1.4" />
      <circle cx="19" cy="12" r="1.4" />
    </>,
    p.size,
    p.style,
  );

export const IconPlus: Component<IconProps> = (p) =>
  svg(
    <>
      <path d="M12 5v14" />
      <path d="M5 12h14" />
    </>,
    p.size,
    p.style,
  );

export const IconSearch: Component<IconProps> = (p) =>
  svg(
    <>
      <circle cx="11" cy="11" r="7" />
      <path d="M21 21l-4.3-4.3" />
    </>,
    p.size,
    p.style,
  );

export const IconCheck: Component<IconProps> = (p) =>
  svg(<path d="M5 13l4 4L19 7" />, p.size, p.style);

export const IconChevron: Component<IconProps & { open?: boolean }> = (p) =>
  svg(
    p.open ? <path d="M6 9l6 6 6-6" /> : <path d="M9 6l6 6-6 6" />,
    p.size,
    p.style,
  );

export const IconLaunch: Component<IconProps> = (p) =>
  svg(
    <>
      <path d="M13 5H6a2 2 0 0 0-2 2v11a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2v-7" />
      <path d="M15 3h6v6" />
      <path d="M10 14L21 3" />
    </>,
    p.size,
    p.style,
  );

export const IconSettings: Component<IconProps> = (p) =>
  svg(
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </>,
    p.size,
    p.style,
  );
