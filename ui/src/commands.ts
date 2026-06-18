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
