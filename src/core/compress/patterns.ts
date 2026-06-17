import { normalizeLine } from "./normalize.js";

/**
 * Extract a stable pattern from a raw log line by delegating to normalizeLine
 * at aggressive level — the most thorough replacement of volatile tokens.
 *
 * Previously this file duplicated regex logic (<NUM> + <ID>); that is now
 * consolidated in normalize.ts to avoid drift.
 */
export function extractPattern(line: string): string {
  return normalizeLine(line, "aggressive");
}
