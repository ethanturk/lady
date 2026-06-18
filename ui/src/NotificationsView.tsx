import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { Notification } from "./commands";
import { relTime } from "./time";

/** Parse an ISO timestamp to Unix seconds for relTime (best-effort). */
const isoToUnix = (iso: string): number => {
  const t = Date.parse(iso);
  return Number.isNaN(t) ? 0 : Math.floor(t / 1000);
};

/**
 * GitHub notifications inbox (PH4-006). Lists notification threads, opens one in
 * the browser, marks threads read, and polls periodically. The unread count is
 * surfaced to the parent via `onUnread` (drives the tab badge).
 */
const NotificationsView: Component<{
  refreshNonce: number;
  onUnread: (n: number) => void;
}> = (props) => {
  const [notes, setNotes] = createSignal<Notification[]>([]);
  const [err, setErr] = createSignal<string | null>(null);
  const [loading, setLoading] = createSignal(false);

  const load = () => {
    setLoading(true);
    invoke<Notification[]>("github_notifications")
      .then((n) => {
        setNotes(n);
        props.onUnread(n.filter((x) => x.unread).length);
        setErr(null);
      })
      .catch((e) => setErr(String(e)))
      .finally(() => setLoading(false));
  };

  onMount(() => {
    load();
    // Poll every 60s while mounted.
    const timer = setInterval(load, 60_000);
    onCleanup(() => clearInterval(timer));
  });

  const open = (n: Notification) => {
    invoke("open_url", { url: n.url }).catch((e) => setErr(String(e)));
  };

  const markRead = (n: Notification) => {
    invoke("github_mark_read", { id: n.id })
      .then(() => {
        setNotes((prev) => prev.map((x) => (x.id === n.id ? { ...x, unread: false } : x)));
        props.onUnread(notes().filter((x) => x.unread && x.id !== n.id).length);
      })
      .catch((e) => setErr(String(e)));
  };

  const smallBtn = {
    border: "1px solid #ccc",
    background: "#fff",
    "border-radius": "3px",
    "font-size": "0.72rem",
    padding: "0 0.45rem",
    cursor: "pointer",
  };

  return (
    <div style={{ height: "100%", "overflow-y": "auto", padding: "0.85rem 1rem" }}>
      <div style={{ display: "flex", "align-items": "center", gap: "0.5rem", "margin-bottom": "0.5rem" }}>
        <h3 style={{ margin: 0, "font-size": "0.95rem" }}>GitHub notifications</h3>
        <button style={smallBtn} disabled={loading()} onClick={load}>
          {loading() ? "Refreshing…" : "Refresh"}
        </button>
      </div>

      <Show when={err()}>
        <p style={{ color: "crimson", "font-size": "0.85rem" }}>{err()}</p>
      </Show>

      <Show when={notes().length === 0 && !err()}>
        <p style={{ color: "#888", "font-size": "0.85rem" }}>No notifications.</p>
      </Show>

      <ul style={{ margin: 0, padding: 0, "list-style": "none" }}>
        <For each={notes()}>
          {(n) => (
            <li
              style={{
                display: "flex",
                "align-items": "center",
                gap: "0.5rem",
                padding: "0.35rem 0",
                "border-bottom": "1px solid #f0f0f0",
                "font-size": "0.84rem",
              }}
            >
              <span
                style={{
                  width: "7px",
                  height: "7px",
                  "border-radius": "50%",
                  "flex-shrink": "0",
                  background: n.unread ? "#0969da" : "transparent",
                }}
                title={n.unread ? "unread" : "read"}
              />
              <span style={{ color: "#888", "font-size": "0.72rem", "min-width": "8ch" }}>{n.kind}</span>
              <span style={{ flex: "1", "font-weight": n.unread ? 600 : 400, overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }} title={n.title}>
                {n.title}
              </span>
              <span style={{ color: "#666", "font-family": "monospace", "font-size": "0.72rem", "max-width": "16ch", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap" }}>
                {n.repo}
              </span>
              <span style={{ color: "#888", "white-space": "nowrap" }}>{relTime(isoToUnix(n.updated))}</span>
              <button style={smallBtn} onClick={() => open(n)}>Open</button>
              <Show when={n.unread}>
                <button style={smallBtn} onClick={() => markRead(n)}>Mark read</button>
              </Show>
            </li>
          )}
        </For>
      </ul>
    </div>
  );
};

export default NotificationsView;
