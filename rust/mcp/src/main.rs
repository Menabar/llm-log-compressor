//! loglens-mcp — the LogLens MCP server.
//!
//! Compresses a log file into a normalized pattern stream with the native
//! `loglens-core` scanner, writes the result plus a line-number index to
//! `$TMPDIR/loglens/<uuid>.log` + `.json`, and returns
//! `{compressedPath, sidecarPath, summary}`. Conservative tier only.
//!
//! Transport: stdio (what Claude Code spawns).

use std::path::Path;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, Implementation, InitializeResult, ServerCapabilities, ServerInfo,
    },
    tool, tool_handler, tool_router,
    transport::stdio,
    ErrorData as McpError, ServerHandler, ServiceExt,
};

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CompressArgs {
    /// Absolute path to the log file to compress.
    file_path: String,
    /// Normalization tier. Only "conservative" is honored; any other value is
    /// ignored. Optional — defaults to conservative.
    #[serde(default)]
    #[allow(dead_code)]
    level: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
struct LinesArgs {
    /// Path to the .json sidecar returned by compress_log_file.
    sidecar_path: String,
    /// A line from the compressed output (with or without the Nx prefix).
    pattern: String,
}

/// `get_token_savings` takes no arguments.
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct NoArgs {}

#[derive(Clone)]
struct LogLens {
    // Read by the `#[tool_handler]`-generated call_tool/list_tools (dead-code
    // analysis can't see the macro use).
    #[allow(dead_code)]
    tool_router: ToolRouter<LogLens>,
}

#[tool_router]
impl LogLens {
    fn new() -> Self {
        Self { tool_router: Self::tool_router() }
    }

    #[tool(
        description = "Call this when you need to investigate a log file on disk — finding errors, anomalies, or recurring failures — and it's too long to read line by line. It compresses the file into a deduplicated pattern stream with an on-disk line index and returns {compressedPath, sidecarPath, summary}.\n\nWORKFLOW:\n1. Read compressedPath with your file-reading tool.\n2. Scan it for recurring structures, errors, and anomalies — identical lines are collapsed and counted, so an outlier stands out.\n3. When a pattern looks worth investigating, call get_original_lines with the sidecarPath to pull the exact raw lines.\n\nOUTPUT FORMAT:\n- Each line is one deduplicated log line; a leading \"Nx \" means it occurred N times.\n- Non-volatile text (log level, message wording, identifiers) is kept verbatim; only volatile values are replaced with ~-prefixed placeholders: ~TS ~DATE ~TIME ~URL ~EMAIL ~PATH ~UUID ~HASH ~HEX ~MAC ~IP ~DUR ~SIZE ~NUM. ANSI color codes are stripped, and a run of the same placeholder separated only by whitespace or , ; : _ collapses to one (e.g. ~NUM ~NUM -> ~NUM).\n\nUSE CASES:\n- Finding recurring errors and how often they occur across a large log.\n- Spotting anomalies (rare patterns) in high-volume output.\n- Triaging a log before deciding which exact lines to read in full."
    )]
    async fn compress_log_file(
        &self,
        Parameters(args): Parameters<CompressArgs>,
    ) -> Result<CallToolResult, McpError> {
        let file_path = args.file_path;
        // Compute + write artifacts off the async reactor (CPU + rayon + blocking IO).
        let payload = tokio::task::spawn_blocking(move || -> std::io::Result<String> {
            let out = loglens_core::compress_path(Path::new(&file_path), 0)?; // 0 = all cores
            let dir = std::env::temp_dir().join("loglens");
            std::fs::create_dir_all(&dir)?;
            let id = uuid::Uuid::new_v4().to_string();
            let log_path = dir.join(format!("{id}.log"));
            let json_path = dir.join(format!("{id}.json"));
            std::fs::write(&log_path, &out.compressed)?;
            std::fs::write(&json_path, &out.sidecar_json)?;

            // Token-savings accounting (byte-based estimate; no tokenizer).
            let input_bytes = out.input_bytes as u64;
            let compressed_bytes = out.compressed.len() as u64;
            let saved_bytes = input_bytes.saturating_sub(compressed_bytes);
            let _ = update_stats(&dir, input_bytes, compressed_bytes); // best-effort

            let payload = serde_json::json!({
                "compressedPath": log_path.to_string_lossy(),
                "sidecarPath": json_path.to_string_lossy(),
                "summary": {
                    "totalLines": out.total_lines,
                    "uniqueGroups": out.unique_groups,
                    "compressionRatio": out.compression_ratio,
                    "inputBytes": input_bytes,
                    "compressedBytes": compressed_bytes,
                    "estTokensSaved": saved_bytes / BYTES_PER_TOKEN,
                }
            });
            Ok(payload.to_string())
        })
        .await
        .map_err(|e| McpError::internal_error(format!("task join: {e}"), None))?
        .map_err(|e| McpError::internal_error(format!("compress failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(payload)]))
    }

    #[tool(
        description = "Expand one compressed pattern back to the original log. Call this after compress_log_file when a pattern looks worth investigating. Pass the sidecarPath it returned and the pattern line copied from the compressed output (the leading \"Nx \" prefix is optional). Returns the matching original line numbers along with the count and a few sample lines, so you can read just that window of the source file instead of the whole log."
    )]
    async fn get_original_lines(
        &self,
        Parameters(args): Parameters<LinesArgs>,
    ) -> Result<CallToolResult, McpError> {
        let LinesArgs { sidecar_path, pattern } = args;
        let payload = tokio::task::spawn_blocking(move || -> std::io::Result<String> {
            let raw = std::fs::read_to_string(&sidecar_path)?;
            let sidecar: serde_json::Value = serde_json::from_str(&raw)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

            // Strip a leading "Nx " count prefix if the LLM passed it verbatim.
            let pat = strip_count_prefix(&pattern);
            let payload = match sidecar.get(pat) {
                Some(e) if !e.is_null() => {
                    let count = e.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                    let n_lines = e
                        .get("lineNumbers")
                        .and_then(|v| v.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0) as u64;
                    serde_json::json!({
                        "pattern": e.get("pattern"),
                        "count": count,
                        "lineNumbers": e.get("lineNumbers"),
                        "truncated": count > n_lines,
                        "samples": e.get("samples"),
                    })
                }
                _ => serde_json::json!({
                    "error": "Pattern not found in sidecar. Check that the pattern matches exactly (after stripping any leading Nx prefix).",
                    "pattern": pat,
                }),
            };
            Ok(payload.to_string())
        })
        .await
        .map_err(|e| McpError::internal_error(format!("task join: {e}"), None))?
        .map_err(|e| McpError::internal_error(format!("read sidecar: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(payload)]))
    }

    #[tool(
        description = "Report the cumulative token savings from compress_log_file calls in this environment. Returns running totals { compressions, rawBytes, compressedBytes, savedBytes, estTokensSaved }. estTokensSaved is an estimate (~4 bytes per token), not an exact tokenizer count. Counters persist in the temp dir and reset only when the OS clears it."
    )]
    async fn get_token_savings(
        &self,
        Parameters(_): Parameters<NoArgs>,
    ) -> Result<CallToolResult, McpError> {
        let payload = tokio::task::spawn_blocking(move || -> String {
            let dir = std::env::temp_dir().join("loglens");
            let (compressions, raw, comp) = read_stats(&dir.join("stats.json"));
            let saved = raw.saturating_sub(comp);
            serde_json::json!({
                "compressions": compressions,
                "rawBytes": raw,
                "compressedBytes": comp,
                "savedBytes": saved,
                "estTokensSaved": saved / BYTES_PER_TOKEN,
                "note": "estTokensSaved is approximate (~4 bytes/token), not a tokenizer count",
            })
            .to_string()
        })
        .await
        .map_err(|e| McpError::internal_error(format!("task join: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(payload)]))
    }
}

#[tool_handler]
impl ServerHandler for LogLens {
    fn get_info(&self) -> ServerInfo {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("loglens-mcp", "0.1.0"))
            .with_instructions(
                "LogLens compresses a log file into a normalized pattern stream so you \
                 can spot anomalies without reading every line, then expands only the \
                 relevant original lines on demand.",
            )
    }
}

/// Rough bytes-per-token divisor for the tokenizer-free savings estimate.
const BYTES_PER_TOKEN: u64 = 4;

/// Read cumulative `{compressions, rawBytes, compressedBytes}` from the stats
/// file; a missing or corrupt file yields zeros.
fn read_stats(path: &Path) -> (u64, u64, u64) {
    let v: serde_json::Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null);
    let g = |k: &str| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
    (g("compressions"), g("rawBytes"), g("compressedBytes"))
}

/// Best-effort read-modify-write of `$TMPDIR/loglens/stats.json` accumulating one
/// compression's raw/compressed byte totals. Callers ignore the result so a stats
/// failure never breaks a compression.
fn update_stats(dir: &Path, raw: u64, compressed: u64) -> std::io::Result<()> {
    let path = dir.join("stats.json");
    let (compressions, raw_total, comp_total) = read_stats(&path);
    let raw_total = raw_total + raw;
    let comp_total = comp_total + compressed;
    let saved = raw_total.saturating_sub(comp_total);
    let payload = serde_json::json!({
        "compressions": compressions + 1,
        "rawBytes": raw_total,
        "compressedBytes": comp_total,
        "savedBytes": saved,
        "estTokensSaved": saved / BYTES_PER_TOKEN,
    });
    std::fs::write(&path, payload.to_string())
}

/// Mirror JS `pattern.replace(/^\d+x\s+/, "")`: drop a leading "<digits>x<ws>".
fn strip_count_prefix(s: &str) -> &str {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && i < b.len() && b[i] == b'x' {
        let mut j = i + 1;
        let start_ws = j;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        if j > start_ws {
            return &s[j..];
        }
    }
    s
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let service = LogLens::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
