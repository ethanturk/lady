export interface AppInfo {
  name: string;
  version: string;
}

/** Opaque handle minted by open_repo; serializes as a plain string. */
export type RepoId = string;

/** Mirrors lady_proto::RefKind. Serde serializes unit variants as strings. */
export type RefKind = "Branch" | "Tag" | "Remote" | "Head";

/** Mirrors lady_proto::RefInfo. */
export interface RefInfo {
  name: string;
  kind: RefKind;
  /** Hex OID string (mirrors lady_proto::Oid which serializes as string). */
  target: string;
}

/** Mirrors lady_proto::Signature. */
export interface Signature {
  name: string;
  email: string;
}

/** Mirrors lady_proto::CommitMeta. */
export interface CommitMeta {
  oid: string;
  parents: string[];
  author: Signature;
  committer: Signature;
  summary: string;
  /** Unix seconds (committer time). */
  time: number;
}

/** Parameters for the walk_log command. */
export interface WalkLogQuery {
  start?: string;
  limit: number;
}

/** A single graph edge (canvas line segment). */
export interface EdgeData {
  from_lane: number;
  to_lane: number;
}

/** Combined commit metadata + graph layout row for the hybrid renderer. */
export interface CommitGraphRow {
  oid: string;
  parents: string[];
  author_name: string;
  summary: string;
  time: number;
  lane: number;
  num_lanes: number;
  edges: EdgeData[];
  refs: string[];
}

/** Result of walk_log_graph — rows plus opaque lane state for next page. */
export interface WalkLogGraphResult {
  rows: CommitGraphRow[];
  /** Opaque lane state; pass as layout_state on the next page request. */
  layout_state: (string | null)[];
}

// ── Diff types ────────────────────────────────────────────────────────────────

export type LineKind = "Added" | "Deleted" | "Context";

export interface DiffLine {
  kind: LineKind;
  content: string;
}

export interface DiffHunk {
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  lines: DiffLine[];
}

export type FileDiffKind = "Added" | "Deleted" | "Modified" | "Binary" | "Image";

export interface FileDiff {
  path: string;
  old_path: string | null;
  kind: FileDiffKind;
  hunks: DiffHunk[];
  old_image_b64: string | null;
  new_image_b64: string | null;
}

// ── Blame types ───────────────────────────────────────────────────────────────

export interface BlameLine {
  line_no: number;
  commit: string;
  author: string;
  time: number;
  content: string;
}

export interface Blame {
  path: string;
  lines: BlameLine[];
}

/**
 * Mirrors lady_git::DiffSpec for the diff_spec command. `value` is a commit oid
 * (Commit) or a file path (WorkingVsIndex = unstaged, IndexVsHead = staged).
 */
export interface DiffSpec {
  kind: "Commit" | "WorkingVsIndex" | "IndexVsHead";
  value: string;
}

// ── Working-tree status types ───────────────────────────────────────────────────

/** Mirrors lady_proto::ChangeKind. */
export type ChangeKind =
  | "Added"
  | "Modified"
  | "Deleted"
  | "Renamed"
  | "Untracked"
  | "Conflicted";

/** Mirrors lady_proto::FileStatus. */
export interface FileStatus {
  path: string;
  old_path: string | null;
  kind: ChangeKind;
}

/** Mirrors lady_proto::WorkingTree. */
export interface WorkingTree {
  staged: FileStatus[];
  unstaged: FileStatus[];
  untracked: string[];
}

/** Mirrors lady_proto::AheadBehind. */
export interface AheadBehind {
  ahead: number;
  behind: number;
}

/** Mirrors lady_proto::StashEntry. */
export interface StashEntry {
  index: number;
  message: string;
  oid: string;
}

/** Mirrors lady_proto::FfMode. */
export type FfMode = "Auto" | "Only" | "Never";

/** Mirrors lady_proto::MergeOutcome. */
export type MergeOutcome =
  | { kind: "AlreadyUpToDate" }
  | { kind: "FastForwarded" }
  | { kind: "Merged"; value: string }
  | { kind: "Conflicts"; value: string[] };

/** Mirrors lady_proto::ApplyOutcome. */
export type ApplyOutcome =
  | { kind: "Applied"; value: string }
  | { kind: "Conflicts"; value: string[] };

// ── Repository-manager types ───────────────────────────────────────────────────

export interface RecentRepo {
  path: string;
  group: string | null;
}

export interface Settings {
  recent: RecentRepo[];
}

/** An opened repo backing one tab in the repository manager (UI-only). */
export interface OpenRepo {
  path: string;
  id: RepoId;
  group: string | null;
  dirty: boolean;
}
