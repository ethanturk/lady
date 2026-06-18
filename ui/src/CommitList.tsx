import { createMemo, createSignal, For, onMount, Show } from "solid-js";
import type { Component } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { CommitMeta, RepoId, WalkLogQuery } from "./commands";

const ROW_H = 48;
const BATCH = 200;
const BUFFER = 5;
const LOAD_AHEAD_PX = 600;

function relTime(secs: number): string {
  const diff = Math.floor(Date.now() / 1000) - secs;
  if (diff < 60) return "just now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  if (diff < 2592000) return `${Math.floor(diff / 86400)}d ago`;
  return new Date(secs * 1000).toLocaleDateString();
}

const CommitList: Component<{ repoId: RepoId }> = (props) => {
  const [commits, setCommits] = createSignal<CommitMeta[]>([]);
  const [scrollTop, setScrollTop] = createSignal(0);
  const [containerH, setContainerH] = createSignal(400);
  const [loading, setLoading] = createSignal(false);
  const [hasMore, setHasMore] = createSignal(true);
  const [cursor, setCursor] = createSignal<string | undefined>(undefined);

  let container!: HTMLDivElement;

  const totalH = () => commits().length * ROW_H;
  const startRow = () => Math.max(0, Math.floor(scrollTop() / ROW_H) - BUFFER);
  const endRow = () =>
    Math.min(commits().length, Math.ceil((scrollTop() + containerH()) / ROW_H) + BUFFER);
  const visibleSlice = createMemo(() => commits().slice(startRow(), endRow()));

  const loadMore = async () => {
    if (loading() || !hasMore()) return;
    setLoading(true);
    try {
      const cur = cursor();
      const q: WalkLogQuery = { start: cur, limit: cur ? BATCH + 1 : BATCH };
      const batch = await invoke<CommitMeta[]>("walk_log", {
        repo: props.repoId,
        query: q,
      });
      // When paginating, the first result is the cursor commit already loaded —
      // skip it to avoid duplicates.
      const fresh = cur ? batch.slice(1) : batch;
      setCommits((prev) => [...prev, ...fresh]);
      setHasMore(fresh.length === BATCH);
      if (fresh.length > 0) setCursor(fresh[fresh.length - 1].oid);
    } finally {
      setLoading(false);
    }
  };

  onMount(() => {
    setContainerH(container.clientHeight || 400);
    loadMore();
  });

  const onScroll = () => {
    const st = container.scrollTop;
    setScrollTop(st);
    if (st + containerH() >= totalH() - LOAD_AHEAD_PX) loadMore();
  };

  return (
    <div style={{ display: "flex", "flex-direction": "column", height: "100%" }}>
      <div
        ref={container}
        style={{ flex: "1", "overflow-y": "auto", position: "relative" }}
        onScroll={onScroll}
      >
        <div style={{ height: `${totalH()}px`, position: "relative" }}>
          <div
            style={{
              position: "absolute",
              top: `${startRow() * ROW_H}px`,
              left: 0,
              right: 0,
            }}
          >
            <For each={visibleSlice()}>
              {(c) => (
                <div
                  style={{
                    height: `${ROW_H}px`,
                    display: "flex",
                    "align-items": "center",
                    gap: "0.75rem",
                    padding: "0 0.75rem",
                    "border-bottom": "1px solid var(--border)",
                    "box-sizing": "border-box",
                    "font-size": "0.875rem",
                  }}
                >
                  <span
                    style={{
                      "font-family": "monospace",
                      color: "var(--fg-muted)",
                      "min-width": "6.5ch",
                    }}
                  >
                    {c.oid.slice(0, 8)}
                  </span>
                  <span
                    style={{
                      flex: "1",
                      overflow: "hidden",
                      "text-overflow": "ellipsis",
                      "white-space": "nowrap",
                    }}
                  >
                    {c.summary}
                  </span>
                  <span
                    style={{
                      color: "var(--fg-muted)",
                      "white-space": "nowrap",
                      "max-width": "14ch",
                      overflow: "hidden",
                      "text-overflow": "ellipsis",
                    }}
                  >
                    {c.author.name}
                  </span>
                  <span
                    style={{
                      color: "var(--fg-muted)",
                      "white-space": "nowrap",
                      "min-width": "7ch",
                      "text-align": "right",
                    }}
                  >
                    {relTime(c.time)}
                  </span>
                </div>
              )}
            </For>
          </div>
        </div>
      </div>
      <Show when={loading()}>
        <div
          style={{
            padding: "0.4rem",
            "text-align": "center",
            color: "var(--fg-muted)",
            "font-size": "0.8rem",
            "flex-shrink": 0,
          }}
        >
          Loading…
        </div>
      </Show>
    </div>
  );
};

export default CommitList;
