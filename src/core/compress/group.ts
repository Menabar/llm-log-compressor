import { Aggressiveness, normalizeLine } from "./normalize.js";
import { StackItem } from "./stacktrace.js";

/** A group of log lines that share the same normalized pattern. */
export type Group = {
  /** The normalized pattern string used as the grouping key. */
  pattern: string;
  /** Total number of raw lines that matched this pattern. */
  count: number;
  /**
   * Up to 3 representative raw samples.
   * For collapsed stack traces the full frame block is joined with "\n".
   */
  samples: string[];
};

/**
 * Group pre-processed log items by their normalized form.
 *
 * @param items - Output of collapseStackTraces (or any compatible array).
 * @param level - Normalization aggressiveness for key derivation.
 * @returns     Ordered array of Groups (insertion order of first occurrence).
 */
export function groupLines(
  items: StackItem[],
  level: Aggressiveness
): Group[] {
  const map = new Map<string, Group>();

  for (const item of items) {
    const key = normalizeLine(item.line, level);

    if (!map.has(key)) {
      map.set(key, { pattern: key, count: 0, samples: [] });
    }

    const group = map.get(key)!;
    group.count++;

    if (group.samples.length < 3) {
      // For collapsed stacks, join the raw frame lines so the sample is
      // self-contained and the expand step can reconstruct the full trace.
      const sample =
        item.raw.length === 1 ? item.raw[0] : item.raw.join("\n");
      group.samples.push(sample);
    }
  }

  return Array.from(map.values());
}
