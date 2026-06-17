// ORDERING IS LOAD-BEARING.
// Rules must run most-specific first, bare-number <NUM> catch-all LAST.
// If <NUM> ran earlier it would clobber the digit sequences that UUID, IP,
// HASH, HEX, MAC, DUR, SIZE and DATE/TIME patterns depend on to match.

export type Aggressiveness = "conservative" | "moderate" | "aggressive";

const LEVEL_RANK: Record<Aggressiveness, number> = {
  conservative: 0,
  moderate: 1,
  aggressive: 2,
};

interface Rule {
  name: string;
  level: Aggressiveness;
  pattern: RegExp;
  replacement: string;
  why: string;
}

const RULES: Rule[] = [
  // ── Conservative tier ────────────────────────────────────────────────────
  {
    name: "ANSI",
    level: "conservative",
    pattern: /\x1b\[[0-9;]*m/g,
    replacement: "",
    why: "Strips terminal colour codes — purely cosmetic, zero semantic value.",
  },
  {
    name: "TS",
    level: "conservative",
    // ISO-8601 datetime: must run before DATE and TIME so their digit sequences
    // are consumed as a unit rather than matched piecemeal.
    pattern: /\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})?/g,
    replacement: "<TS>",
    why: "Full ISO-8601 timestamps are fully volatile — replace before DATE/TIME rules fire.",
  },
  {
    name: "DATE",
    level: "conservative",
    // Two common date formats: YYYY-MM-DD and MM/DD/YYYY.
    pattern: /\d{4}-\d{2}-\d{2}|\d{2}\/\d{2}\/\d{4}/g,
    replacement: "<DATE>",
    why: "Date tokens change every day; collapsing them merges log lines from different days.",
  },
  {
    name: "TIME",
    level: "conservative",
    pattern: /\d{2}:\d{2}:\d{2}(\.\d+)?/g,
    replacement: "<TIME>",
    why: "Wall-clock times are volatile; stripping them keeps lines comparable across runs.",
  },
  {
    name: "URL",
    level: "conservative",
    // Must run before PATH: a URL like https://host/a/b would otherwise be
    // partially consumed by the PATH rule leaving a dangling protocol prefix.
    pattern: /https?:\/\/\S+/g,
    replacement: "<URL>",
    why: "Full URLs contain volatile query params, tokens, and host names.",
  },
  {
    name: "EMAIL",
    level: "conservative",
    pattern: /\S+@\S+\.\S+/g,
    replacement: "<EMAIL>",
    why: "Email addresses are PII and high-cardinality; normalize to a stable placeholder.",
  },
  {
    name: "PATH",
    level: "conservative",
    // Unix: two-or-more path segments (avoids matching lone /dev).
    // Windows: drive-letter prefix.
    // Must run after URL so URL paths are already replaced.
    pattern: /(?:\/[\w.-]+){2,}|[A-Za-z]:\\[^\s]+/g,
    replacement: "<PATH>",
    why: "File-system paths carry volatile line numbers, temp dirs, and user-specific prefixes.",
  },
  {
    name: "UUID",
    level: "conservative",
    // Canonical 8-4-4-4-12 UUID; case-insensitive.
    // Must run before HASH so the hex segments aren't mistaken for a bare hash.
    pattern: /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi,
    replacement: "<UUID>",
    why: "UUIDs are by definition unique per-object — replace to collapse identical events.",
  },
  {
    name: "HASH",
    level: "conservative",
    // Lowercase hex run of 16+ chars (git SHAs, content hashes).
    // Must run after UUID so UUID segments are already replaced.
    pattern: /\b[0-9a-f]{16,}\b/gi,
    replacement: "<HASH>",
    why: "Long hex strings are cryptographic hashes or IDs — always volatile.",
  },
  {
    name: "HEX",
    level: "conservative",
    // 0x-prefixed hex literal.
    pattern: /0x[0-9a-fA-F]+/g,
    replacement: "<HEX>",
    why: "Hex literals (addresses, bitmasks) change between runs; safe to normalize.",
  },
  {
    name: "MAC",
    level: "conservative",
    // Six colon-separated two-hex-digit groups; case-insensitive.
    pattern: /(?:[0-9a-f]{2}:){5}[0-9a-f]{2}/gi,
    replacement: "<MAC>",
    why: "MAC addresses identify hardware — high cardinality, zero diagnostic value.",
  },
  {
    name: "IP",
    level: "conservative",
    // IPv4 with optional :port suffix.
    // Must run after UUID/HASH/HEX so dotted-decimal isn't consumed piecemeal.
    pattern: /\b\d{1,3}(?:\.\d{1,3}){3}\b(?::\d+)?/g,
    replacement: "<IP>",
    why: "IP addresses and ports change per deployment/request — collapse to stable token.",
  },
  {
    name: "DUR",
    level: "conservative",
    // Duration literals: 10ms, 3.5s, 100us, etc.
    // Must run before <NUM> so the unit suffix is consumed together with the digits.
    pattern: /\b\d+(?:\.\d+)?(?:ns|us|ms|s|m|h)\b/g,
    replacement: "<DUR>",
    why: "Timing values are volatile; keep them from fragmenting into bare numbers.",
  },
  {
    name: "SIZE",
    level: "conservative",
    // Byte sizes: 1.5 MB, 200KB, 4GiB, etc. (with optional space before unit).
    // Must run before <NUM> so value+unit are replaced atomically.
    pattern: /\b\d+(?:\.\d+)?\s?(?:B|KB|MB|GB|TB|KiB|MiB|GiB)\b/g,
    replacement: "<SIZE>",
    why: "Byte counts vary per request/chunk; normalizing avoids spurious group splits.",
  },
  // <NUM> is intentionally LAST in the conservative tier — it is a catch-all
  // that must not fire before the more-specific patterns above have had a chance
  // to consume their digit sequences.
  {
    name: "NUM",
    level: "conservative",
    pattern: /\d+(?:\.\d+)?/g,
    replacement: "<NUM>",
    why: "Catch-all for any remaining bare numeric literal (IDs, counts, line numbers).",
  },

  // ── Moderate tier ────────────────────────────────────────────────────────
  {
    name: "STR_DOUBLE",
    level: "moderate",
    // Quoted string values (double-quote variant).
    // Placed before key=value so string values inside k=v pairs are also caught.
    pattern: /"[^"]*"/g,
    replacement: "<STR>",
    why: "Double-quoted string values (messages, tokens) are high-cardinality.",
  },
  {
    name: "STR_SINGLE",
    level: "moderate",
    pattern: /'[^']*'/g,
    replacement: "<STR>",
    why: "Single-quoted string values carry the same high-cardinality concern.",
  },
  {
    name: "KV",
    level: "moderate",
    // key=value: keep the key for semantic signal, replace RHS with <VAL>.
    pattern: /(\w+=)\S+/g,
    replacement: "$1<VAL>",
    why: "Key=value RHS tokens (session IDs, tokens, paths) are volatile; key is semantic.",
  },

  // ── Aggressive tier ──────────────────────────────────────────────────────
  {
    name: "ID",
    level: "aggressive",
    // Identifiers that mix letters and digits (camelCase IDs, generated names).
    pattern: /\b[A-Za-z]+\d\w*\b/g,
    replacement: "<ID>",
    why: "Mixed alnum identifiers are typically generated IDs — replace for tighter grouping.",
  },
  {
    name: "OPAQUE",
    level: "aggressive",
    // Long opaque mixed-alnum tokens (≥12 chars, not already replaced).
    pattern: /\b[A-Za-z0-9]{12,}\b/g,
    replacement: "<ID>",
    why: "Long alnum tokens are likely encoded IDs or hashes not caught by earlier rules.",
  },
];

/**
 * Normalize a single log line by replacing volatile tokens with stable
 * placeholders so that semantically-identical lines collapse to the same key.
 *
 * Semantic tokens (ERROR, Exception, failed, message text) are left intact so
 * error signal is preserved for LLM consumption.
 *
 * @param line  - Raw log line.
 * @param level - How aggressively to normalize (default: "conservative").
 */
export function normalizeLine(
  line: string,
  level: Aggressiveness = "conservative"
): string {
  const maxRank = LEVEL_RANK[level];
  return RULES.filter((r) => LEVEL_RANK[r.level] <= maxRank).reduce(
    (acc, rule) => acc.replace(rule.pattern, rule.replacement),
    line
  );
}
