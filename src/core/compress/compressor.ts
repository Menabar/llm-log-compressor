import { Aggressiveness } from "./normalize.js";
import { collapseStackTraces } from "./stacktrace.js";
import { groupLines, Group } from "./group.js";

export type { Group };

export interface CompressResult {
  groups: Group[];
  summary: {
    totalLines: number;
    uniqueGroups: number;
    /** Ratio of unique groups to total lines (lower = more compression). */
    compressionRatio: number;
  };
}

/**
 * Compress a raw multi-line log string into a grouped, de-duplicated summary.
 *
 * Pipeline:
 *   1. Split into lines.
 *   2. Pre-pass: collapse consecutive stack-frame runs into synthetic entries.
 *   3. Group by normalized pattern, counting occurrences and keeping samples.
 *   4. Return groups + summary stats.
 *
 * @param raw     - The raw log text (newline-separated).
 * @param options - `level` controls normalization aggressiveness (default "conservative").
 */
export function compressLogs(
  raw: string,
  options: { level?: Aggressiveness } = {}
): CompressResult {
  const level = options.level ?? "conservative";

  const lines = raw.split("\n");
  const items = collapseStackTraces(lines);
  const groups = groupLines(items, level);

  const totalLines = lines.length;
  const uniqueGroups = groups.length;
  const compressionRatio = totalLines === 0 ? 0 : uniqueGroups / totalLines;

  return {
    groups,
    summary: {
      totalLines,
      uniqueGroups,
      compressionRatio,
    },
  };
}
