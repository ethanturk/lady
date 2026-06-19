import { createSignal, For, Show } from "solid-js";
import type { Component, JSX } from "solid-js";

/**
 * One entry in a context menu. A bare `"divider"` draws a separator; otherwise a
 * row with an optional right-aligned `shortcut`, a `submenu` that opens on hover,
 * or a `run` action. `disabled` greys it out; `danger` paints it red.
 */
export type MenuEntry =
  | "divider"
  | {
      label: string;
      shortcut?: string;
      danger?: boolean;
      disabled?: boolean;
      submenu?: MenuEntry[];
      run?: () => void | Promise<void>;
    };

interface ContextMenuProps {
  x: number;
  y: number;
  items: MenuEntry[];
  onClose: () => void;
}

const PANEL: JSX.CSSProperties = {
  "z-index": "999",
  "min-width": "230px",
  background: "var(--pill)",
  border: "1px solid var(--bd)",
  "border-radius": "9px",
  padding: "5px",
  "box-shadow": "0 14px 38px rgba(0,0,0,0.45)",
  "font-size": "12.5px",
};

/** A single hoverable row (handles its own nested submenu panel). */
const Row: Component<{ item: Exclude<MenuEntry, "divider">; onClose: () => void }> = (props) => {
  const [open, setOpen] = createSignal(false);
  let rowEl: HTMLButtonElement | undefined;
  const hasSub = () => !!props.item.submenu && props.item.submenu.length > 0;

  const fire = () => {
    if (props.item.disabled) return;
    if (hasSub()) return; // submenu parents don't act on click
    props.onClose();
    void props.item.run?.();
  };

  return (
    <div style={{ position: "relative" }} onMouseEnter={() => setOpen(true)} onMouseLeave={() => setOpen(false)}>
      <button
        ref={rowEl}
        class="hov"
        role="menuitem"
        disabled={props.item.disabled}
        onClick={fire}
        style={{
          display: "flex",
          "align-items": "center",
          width: "100%",
          gap: "10px",
          padding: "7px 11px",
          border: "none",
          background: "transparent",
          "border-radius": "6px",
          color: props.item.danger ? "var(--badge-d)" : "var(--tx)",
          opacity: props.item.disabled ? 0.4 : 1,
          cursor: props.item.disabled ? "default" : "pointer",
          "text-align": "left",
        }}
      >
        <span style={{ flex: "1", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }}>
          {props.item.label}
        </span>
        <Show when={props.item.shortcut}>
          <span style={{ color: "var(--tx3)", "font-size": "11.5px", "white-space": "nowrap" }}>{props.item.shortcut}</span>
        </Show>
        <Show when={hasSub()}>
          <span style={{ color: "var(--tx3)" }}>▸</span>
        </Show>
      </button>
      {/* Nested submenu, opened on hover; clamped so it never leaves the right edge. */}
      <Show when={hasSub() && open() && !props.item.disabled}>
        <div
          role="menu"
          style={{
            ...PANEL,
            position: "absolute",
            top: "-5px",
            left: "100%",
            "margin-left": "2px",
            ...(rowEl && rowEl.getBoundingClientRect().right + 240 > window.innerWidth
              ? { left: "auto", right: "100%", "margin-left": "0", "margin-right": "2px" }
              : {}),
          }}
        >
          <For each={props.item.submenu}>
            {(sub) => (
              <Show when={sub !== "divider"} fallback={<div style={{ height: "1px", background: "var(--bd)", margin: "5px 8px" }} />}>
                <Row item={sub as Exclude<MenuEntry, "divider">} onClose={props.onClose} />
              </Show>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
};

/**
 * Generic fixed-position context menu with submenus, shortcut hints, dividers,
 * disabled/danger rows. A transparent backdrop closes it (click or right-click),
 * Esc closes it. Position is clamped on-screen.
 */
const ContextMenu: Component<ContextMenuProps> = (props) => {
  const x = () => Math.min(props.x, window.innerWidth - 250);
  const y = () => Math.min(props.y, window.innerHeight - Math.min(props.items.length * 32 + 20, window.innerHeight - 20));

  return (
    <>
      <div
        style={{ position: "fixed", inset: "0", "z-index": "998" }}
        onClick={() => props.onClose()}
        onContextMenu={(e) => {
          e.preventDefault();
          props.onClose();
        }}
      />
      <div
        role="menu"
        ref={(el) => queueMicrotask(() => el.focus())}
        tabindex={-1}
        onKeyDown={(e) => {
          if (e.key === "Escape") props.onClose();
        }}
        style={{ ...PANEL, position: "fixed", left: `${x()}px`, top: `${Math.max(y(), 8)}px`, outline: "none" }}
      >
        <For each={props.items}>
          {(item) => (
            <Show when={item !== "divider"} fallback={<div style={{ height: "1px", background: "var(--bd)", margin: "5px 8px" }} />}>
              <Row item={item as Exclude<MenuEntry, "divider">} onClose={props.onClose} />
            </Show>
          )}
        </For>
      </div>
    </>
  );
};

export default ContextMenu;
