/**
 * Deterministic author avatar color + initials, shared by the commit graph
 * nodes, the history-list avatars, and the commit detail pane so the same
 * author reads the same everywhere (design uses colored initial avatars).
 */
export function authorColor(name: string): string {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) | 0;
  const hue = Math.abs(h) % 360;
  // Mid lightness / moderate saturation so dark initials read on both themes.
  return `hsl(${hue}, 52%, 62%)`;
}

export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}
