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

/**
 * Mirrors lady_proto::RepoSettings — the settings that can be set globally and
 * overridden per repo. A `null`/absent field means "inherit".
 */
export interface RepoSettings {
  sign?: boolean | null;
  ff?: FfMode | null;
  base_branch?: string | null;
  ai_model?: string | null;
}

/** Mirrors the backend `ResolvedRepoSettings` (the three layers in one call). */
export interface ResolvedRepoSettings {
  /** override ?? global (built-in fallbacks applied client-side). */
  effective: RepoSettings;
  /** This repo's raw override block. */
  override: RepoSettings;
  /** The global defaults. */
  global: RepoSettings;
}

/** Mirrors lady_proto::GitIdentity (local `.git/config` user.name/email). */
export interface GitIdentity {
  name?: string | null;
  email?: string | null;
}

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

/** Mirrors lady_proto::RebaseOutcome. */
export type RebaseOutcome =
  | { kind: "Rebased" }
  | { kind: "Conflicts"; value: string[] }
  | { kind: "Stopped" };

/** Mirrors lady_proto::RebaseAction. */
export type RebaseAction = "Pick" | "Reword" | "Edit" | "Squash" | "Fixup" | "Drop";

/** Mirrors lady_proto::RebaseStep. */
export interface RebaseStep {
  oid: string;
  action: RebaseAction;
  message: string | null;
}

/** Mirrors lady_proto::SignatureStatus. */
export type SignatureStatus = "Good" | "Bad" | "Untrusted" | "None";

/** Mirrors lady_license::LicenseStatus (serde tag "kind"). */
export type LicenseStatus =
  | { kind: "Trial"; days_left: number }
  | { kind: "Expired" }
  | { kind: "Licensed"; licensee: string };

/** Mirrors lady_hosting::ForgeKind. */
export type ForgeKind = "GitHub" | "GitLab" | "Bitbucket" | "AzureDevOps";

/** Human label per forge (UI copy). */
export const FORGE_LABEL: Record<ForgeKind, string> = {
  GitHub: "GitHub",
  GitLab: "GitLab",
  Bitbucket: "Bitbucket",
  AzureDevOps: "Azure DevOps",
};

/** Whether a forge calls them "merge requests" (GitLab) vs "pull requests". */
export const requestNoun = (kind: ForgeKind | null): string =>
  kind === "GitLab" ? "merge request" : "pull request";

/** Mirrors lady_hosting::RepoSlug. */
export interface RepoSlug {
  owner: string;
  repo: string;
  project?: string | null;
}

/** Mirrors the HostingInfo DTO (forge-aware connection status). */
export interface HostingInfo {
  kind: ForgeKind | null;
  connected: boolean;
  login: string | null;
  slug: RepoSlug | null;
}

/** Mirrors lady_hosting::RepoInfo (created remote repo URLs). */
export interface RepoInfo {
  clone_url: string;
  web_url: string;
}

/** Mirrors lady_hosting::Notification (GitHub). */
export interface Notification {
  id: string;
  title: string;
  repo: string;
  kind: string;
  url: string;
  unread: boolean;
  updated: string;
}

/** All forges, for selectors. */
export const FORGE_KINDS: ForgeKind[] = ["GitHub", "GitLab", "Bitbucket", "AzureDevOps"];

/** Mirrors lady_proto::Worktree. */
export interface Worktree {
  path: string;
  branch: string | null;
  head: string | null;
  locked: boolean;
}

/** Mirrors lady_proto::ReflogEntry. */
export interface ReflogEntry {
  oid: string;
  prev_oid: string;
  action: string;
  message: string;
  time: number;
}

/** Mirrors lady_proto::BisectState. */
export interface BisectState {
  current_oid: string | null;
  remaining_steps_estimate: number;
  suspected: string | null;
}

/** Mirrors lady_proto::LfsFile. */
export interface LfsFile {
  path: string;
  oid: string;
  downloaded: boolean;
}

/** Mirrors lady_proto::LfsStatus. */
export interface LfsStatus {
  available: boolean;
  patterns: string[];
  files: LfsFile[];
}

/** Mirrors lady_proto::Submodule. */
export interface Submodule {
  path: string;
  url: string;
  sha: string;
  initialized: boolean;
  dirty: boolean;
}

/** Mirrors lady_proto::FlowKind. */
export type FlowKind = "Feature" | "Release" | "Hotfix";

/** Mirrors lady_proto::FlowConfig. */
export interface FlowConfig {
  initialized: boolean;
  master: string;
  develop: string;
  feature_prefix: string;
  release_prefix: string;
  hotfix_prefix: string;
  version_tag_prefix: string;
}

/** Mirrors lady_proto::ConflictState. */
export type ConflictState = "None" | "Merge" | "Rebase" | "CherryPick" | "Revert";

/** Mirrors lady_proto::ConflictSides (index stages 1/2/3). */
export interface ConflictSides {
  base: string | null;
  ours: string | null;
  theirs: string | null;
}

/** Mirrors lady_proto::ConflictRegion. */
export interface ConflictRegion {
  ours: string[];
  base: string[];
  theirs: string[];
}

/** Mirrors lady_proto::ConflictSegment (serde tagged). */
export type ConflictSegment =
  | { kind: "Context"; value: string[] }
  | { kind: "Conflict"; value: ConflictRegion };

/** Mirrors lady_proto::ParsedConflict. */
export interface ParsedConflict {
  segments: ConflictSegment[];
  has_base: boolean;
}

// ── Repository-manager types ───────────────────────────────────────────────────

export interface RecentRepo {
  path: string;
  group: string | null;
}

/** Mirrors lady_proto::PlaceholderKind. */
export type PlaceholderKind = "Text" | "Branch" | "File";

/** Mirrors lady_proto::Placeholder. */
export interface Placeholder {
  name: string;
  kind: PlaceholderKind;
}

/** Mirrors lady_proto::CustomCommand. */
export interface CustomCommand {
  name: string;
  template: string;
}

/** Mirrors lady_proto::CommandOutput. */
export interface CommandOutput {
  stdout: string;
  stderr: string;
  exit_code: number;
}

export interface Settings {
  recent: RecentRepo[];
  custom_commands: CustomCommand[];
  /** License key; owned by the licensing commands and preserved on other saves. */
  license?: string | null;
}

/** An opened repo backing one tab in the repository manager (UI-only). */
export interface OpenRepo {
  path: string;
  id: RepoId;
  group: string | null;
  dirty: boolean;
}
