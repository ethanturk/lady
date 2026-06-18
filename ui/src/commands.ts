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
