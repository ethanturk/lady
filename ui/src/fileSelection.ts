export interface FileSelection {
  path: string;
  staged: boolean;
}

export const selectionKey = (sel: FileSelection) => `${sel.staged ? "s" : "u"}\0${sel.path}`;

export const sameSelection = (
  a: FileSelection | null | undefined,
  b: FileSelection | null | undefined,
): boolean => !!a && !!b && a.path === b.path && a.staged === b.staged;

export const resolveFileSelection = (
  order: FileSelection[],
  current: FileSelection[],
  anchor: FileSelection | null,
  clicked: FileSelection,
  mods: { meta: boolean; shift: boolean },
): { selected: FileSelection[]; primary: FileSelection | null; anchor: FileSelection | null } => {
  if (mods.shift && anchor) {
    const ids = order.map(selectionKey);
    const a = ids.indexOf(selectionKey(anchor));
    const b = ids.indexOf(selectionKey(clicked));
    if (a !== -1 && b !== -1) {
      const [lo, hi] = a <= b ? [a, b] : [b, a];
      return { selected: order.slice(lo, hi + 1), primary: clicked, anchor };
    }
  }

  if (mods.meta) {
    const hasClicked = current.some((sel) => sameSelection(sel, clicked));
    const next = hasClicked ? current.filter((sel) => !sameSelection(sel, clicked)) : [...current, clicked];
    return {
      selected: next,
      primary: next.length === 0 ? null : next.some((sel) => sameSelection(sel, clicked)) ? clicked : next[next.length - 1],
      anchor: clicked,
    };
  }

  return { selected: [clicked], primary: clicked, anchor: clicked };
};
