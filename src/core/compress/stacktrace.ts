/**
 * Stack-trace collapse.
 *
 * Consecutive stack-frame lines in a log are individually noisy but
 * collectively they represent a single "location" signal.  This pre-pass
 * collapses a run of N frames into one synthetic entry that carries:
 *   - a stable `line` token  <STACK frames=N top="<first frame trimmed>">
 *   - the original `raw` lines so the expand step can reconstruct them.
 *
 * Supported frame styles
 * ──────────────────────
 *  JS / Java:  lines matching  /^\s+at .+/
 *  Python:     a "Traceback (most recent call last):" header followed by
 *              one or more lines matching  /^\s+File ".*"/
 *
 * Non-frame lines pass through unchanged as { line, raw: [line] }.
 */

/** A single item in the pre-processed log stream. */
export interface StackItem {
  /** Normalized/synthetic line text used for grouping. */
  line: string;
  /** Original raw lines that produced this item (1-to-1 for normal lines). */
  raw: string[];
}

// Regexes that identify individual stack-frame lines.
const JS_JAVA_FRAME = /^\s+at .+/;
const PYTHON_FILE_FRAME = /^\s+File "/;
const PYTHON_TRACEBACK = /^Traceback \(most recent call last\):/;

/**
 * Collapse consecutive stack frames in `lines` into synthetic StackItems.
 *
 * @param lines - Raw log lines (no pre-processing assumed).
 * @returns     Array of StackItems, one per logical log event.
 */
export function collapseStackTraces(lines: string[]): StackItem[] {
  const result: StackItem[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];

    // ── JS / Java frame run ────────────────────────────────────────────────
    if (JS_JAVA_FRAME.test(line)) {
      const frameLines: string[] = [];
      while (i < lines.length && JS_JAVA_FRAME.test(lines[i])) {
        frameLines.push(lines[i]);
        i++;
      }
      result.push(buildStackItem(frameLines));
      continue;
    }

    // ── Python traceback block ─────────────────────────────────────────────
    if (PYTHON_TRACEBACK.test(line)) {
      // Collect the header plus all following "  File ..." frame lines.
      const frameLines: string[] = [line];
      i++;
      while (i < lines.length && PYTHON_FILE_FRAME.test(lines[i])) {
        // Also consume the "    <code line>" that follows each File line.
        frameLines.push(lines[i]);
        i++;
        // The code-snippet line (indented but not a File line) is part of the frame.
        if (i < lines.length && /^\s{4,}/.test(lines[i]) && !PYTHON_FILE_FRAME.test(lines[i])) {
          frameLines.push(lines[i]);
          i++;
        }
      }
      result.push(buildStackItem(frameLines));
      continue;
    }

    // ── Normal line ────────────────────────────────────────────────────────
    result.push({ line, raw: [line] });
    i++;
  }

  return result;
}

/** Build a synthetic StackItem from a collected run of frame lines. */
function buildStackItem(frameLines: string[]): StackItem {
  const n = frameLines.length;
  const top = frameLines[0].trim();
  return {
    line: `<STACK frames=${n} top="${top}">`,
    raw: frameLines,
  };
}
