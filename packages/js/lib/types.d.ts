export interface ActionResult {
  exitCode: number;
  stdout?: string | null;
  stderr?: string | null;
  outputs?: Record<string, string>;
}

export interface CacheStats {
  blobCount: number;
  blobBytes: number;
  actionCount: number;
  actionBytes: number;
}
