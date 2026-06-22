// AI (Phase 5) bindings + a streaming helper. The backend streams token deltas
// over the `ai-stream` Tauri event and resolves the invoke with the full text;
// consent + redaction are enforced server-side (ADR-0009).
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type ProviderKind =
  | "OpenAiCompatible"
  | "OpenAi"
  | "Anthropic"
  | "AnthropicCompatible"
  | "Gemini"
  | "AzureOpenAi"
  | "Mistral";

export const PROVIDERS: ProviderKind[] = [
  "OpenAiCompatible",
  "OpenAi",
  "Anthropic",
  "AnthropicCompatible",
  "Gemini",
  "AzureOpenAi",
  "Mistral",
];

export const PROVIDER_LABEL: Record<ProviderKind, string> = {
  OpenAiCompatible: "OpenAI Compatible",
  OpenAi: "OpenAI",
  Anthropic: "Anthropic Claude",
  AnthropicCompatible: "Anthropic Compatible",
  Gemini: "Google Gemini",
  AzureOpenAi: "Azure OpenAI",
  Mistral: "Mistral",
};

// The compatible endpoints are consent-free, local-first paths (point them only
// at servers you trust); every other provider sends data off-machine.
export const isRemote = (p: ProviderKind): boolean =>
  p !== "OpenAiCompatible" && p !== "AnthropicCompatible";

export interface AiConfig {
  active: ProviderKind | null;
  models: Record<string, string>;
  default_model: string | null;
  openai_base_url: string;
  openai_context_window: number;
  anthropic_base_url: string;
  anthropic_context_window: number;
  azure_endpoint: string;
  azure_deployment: string;
  consented: ProviderKind[];
}

export interface PlannedCommit {
  message: string;
  hunk_ids: string[];
}

export interface CommitPlan {
  commits: PlannedCommit[];
}

/** Mirrors the backend `RecomposePlan` (ai_recompose_plan result). */
export interface RecomposePlan {
  plan: CommitPlan;
  /** Any commit in the span is already pushed (rewriting needs a force-push). */
  pushed: boolean;
  /** How many commits the span currently has (for the "N → M" confirm). */
  commit_count: number;
}

let reqCounter = 0;
function nextReqId(): string {
  reqCounter += 1;
  return `ai-${Date.now()}-${reqCounter}`;
}

/** Whether the error string from a backend AI call means consent is needed. */
export const isConsentError = (msg: string): boolean =>
  msg.includes("consent required");

/**
 * Run a streaming AI command. Subscribes to `ai-stream`, forwards each delta to
 * `onDelta`, and resolves with the full text. `args` must NOT include req_id —
 * it is generated and returned via `setReqId` so the caller can cancel.
 */
export async function runAiStream(
  command: string,
  args: Record<string, unknown>,
  onDelta: (full: string) => void,
  setReqId?: (id: string) => void,
): Promise<string> {
  const reqId = nextReqId();
  setReqId?.(reqId);
  let acc = "";
  const unlisten = await listen<string>("ai-stream", (e) => {
    acc += e.payload;
    onDelta(acc);
  });
  try {
    // The backend returns the full text too; prefer it (handles non-stream).
    const full = await invoke<string>(command, { ...args, reqId });
    return full || acc;
  } finally {
    unlisten();
  }
}

/** Cancel an in-flight streaming completion. */
export const cancelAi = (reqId: string): Promise<void> =>
  invoke("ai_cancel", { reqId });

// Thin config/key/consent wrappers.
export const getAiConfig = (): Promise<AiConfig> => invoke("ai_get_config");
export const setAiConfig = (config: AiConfig): Promise<void> =>
  invoke("ai_set_config", { config });
export const setAiKey = (provider: ProviderKind, key: string): Promise<void> =>
  invoke("ai_set_key", { provider, key });
export const deleteAiKey = (provider: ProviderKind): Promise<void> =>
  invoke("ai_delete_key", { provider });
export const hasAiKey = (provider: ProviderKind): Promise<boolean> =>
  invoke("ai_has_key", { provider });
export const grantConsent = (provider: ProviderKind): Promise<void> =>
  invoke("ai_grant_consent", { provider });
export const revokeConsent = (provider: ProviderKind): Promise<void> =>
  invoke("ai_revoke_consent", { provider });
export const setRepoEnabled = (
  repo: string,
  enabled: boolean,
): Promise<void> => invoke("ai_set_repo_enabled", { repo, enabled });
export const repoEnabled = (repo: string): Promise<boolean> =>
  invoke("ai_repo_enabled", { repo });
export const listModels = (): Promise<string[]> => invoke("ai_list_models");
