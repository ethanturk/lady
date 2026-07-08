import { describe, expect, it } from "vitest";
import { resolveFileSelection, type FileSelection } from "./fileSelection";
import { ltrIsolate } from "./ChangesView";

describe("ltrIsolate", () => {
  // The dir-prefix span renders in a `direction: rtl` box (to pin the ellipsis
  // to the left). Without an LTR isolate the bidi algorithm reorders neutral
  // boundary chars, e.g. `.githooks/` would display as `/githooks.`. The isolate
  // must wrap the string so its internal order stays LTR.
  it("wraps a dotfile dir prefix in U+2066 … U+2069 so rtl doesn't rotate the leading dot", () => {
    expect(ltrIsolate(".githooks/")).toBe("\u2066.githooks/\u2069");
  });

  it("wraps arbitrary paths without altering the inner text", () => {
    expect(ltrIsolate("src/lib/").slice(1, -1)).toBe("src/lib/");
  });
});

const sel = (path: string, staged = false): FileSelection => ({ path, staged });

describe("file selection", () => {
  it("shift-click selects contiguous range between anchor and clicked file", () => {
    const order = [sel("a"), sel("b"), sel("c"), sel("d")];
    const next = resolveFileSelection(order, [sel("b")], sel("b"), sel("d"), { meta: false, shift: true });

    expect(next.selected).toEqual([sel("b"), sel("c"), sel("d")]);
    expect(next.primary).toEqual(sel("d"));
    expect(next.anchor).toEqual(sel("b"));
  });

  it("cmd-click toggles only clicked files into selection", () => {
    const order = [sel("a"), sel("b"), sel("c")];
    const add = resolveFileSelection(order, [sel("a")], sel("a"), sel("c"), { meta: true, shift: false });
    const remove = resolveFileSelection(order, add.selected, sel("c"), sel("a"), { meta: true, shift: false });

    expect(add.selected).toEqual([sel("a"), sel("c")]);
    expect(add.primary).toEqual(sel("c"));
    expect(remove.selected).toEqual([sel("c")]);
    expect(remove.primary).toEqual(sel("c"));
  });

  it("shift-click spans unstaged and staged rows in visible order", () => {
    const order = [sel("u1", false), sel("u2", false), sel("s1", true), sel("s2", true)];
    const next = resolveFileSelection(order, [sel("u2")], sel("u2"), sel("s2", true), { meta: false, shift: true });

    expect(next.selected).toEqual([sel("u2"), sel("s1", true), sel("s2", true)]);
    expect(next.primary).toEqual(sel("s2", true));
  });
});
