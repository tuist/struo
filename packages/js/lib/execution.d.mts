import type { ActionResult, Cache } from "./browser.mjs";

export interface TabTask {
  operation: string;
  inputs: Record<string, string>;
  args: string[];
  memoryBytes: number;
}
export class TabExecutor {
  constructor(options?: { channelName?: string });
  execute(task: TabTask, options?: { signal?: AbortSignal; timeoutMs?: number }): Promise<{ result: ActionResult; workerId: string }>;
  close(): void;
}
export function serveTab(options: {
  channelName?: string;
  memoryLimitBytes: number;
  handlers: Record<string, (task: TabTask, context: { signal: AbortSignal; workerId: string }) => Promise<ActionResult> | ActionResult>;
}): { workerId: string; close(): void };

export interface OAuth2Options {
  authorizationEndpoint: string;
  tokenEndpoint: string;
  revocationEndpoint?: string;
  clientId: string;
  redirectUri: string;
  scopes?: string[];
}
export class OAuth2Client {
  constructor(options: OAuth2Options);
  authorizationURL(): Promise<string>;
  completeAuthorization(callbackUrl?: string): Promise<void>;
  getAccessToken(): Promise<string>;
  logout(): Promise<void>;
}
export interface Pipeline {
  id: number;
  status: string;
  web_url: string;
  sha: string;
  ref: string;
}
export interface PipelineJob {
  id: number;
  name: string;
  status: string;
}
export class GitLabExecutor {
  constructor(options: { url?: string; project: string | number; auth: Pick<OAuth2Client, "getAccessToken"> });
  dispatch(request: { ref: string; variables?: Record<string, string> }, options?: { signal?: AbortSignal }): Promise<Pipeline>;
  get(pipelineId: number, options?: { signal?: AbortSignal }): Promise<Pipeline>;
  cancel(pipelineId: number): Promise<Pipeline>;
  execute(request: { ref: string; variables?: Record<string, string> }, options?: { signal?: AbortSignal; timeoutMs?: number; pollIntervalMs?: number }): Promise<Pipeline>;
  /** Returns at most 100 jobs. */
  jobs(pipelineId: number, options?: { signal?: AbortSignal }): Promise<PipelineJob[]>;
  storeArtifact(options: { jobId: number; path: string; cache: Cache; maxBytes: number; signal?: AbortSignal }): Promise<string>;
}
