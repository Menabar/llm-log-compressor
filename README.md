# LogLens MCP

LogLens is an MCP server that compresses log files before they reach an LLM. Instead of
dumping thousands of raw lines into context, the LLM reads a compact pattern stream, identifies what's interesting, then retrieves only the relevant original
lines.

## Quick Start

To jump into using this tool:
1. Clone the repo and build the binary:
```sh
cd llm-lc/rust
cargo build --release      # produces target/release/loglens-mcp
```
2. Register the server with Claude Code, using the absolute path to the binary you just built:
```sh
claude mcp add loglens -- /absolute/path/to/llm-lc/rust/target/release/loglens-mcp
```
3. Restart Claude Code (or run `/mcp`) to confirm `loglens` is connected, then ask
Claude to investigate a log file — e.g. "use loglens to compress and find the errors
in /var/log/app.log".


## How it works

```
Raw log file

    ↓

Normalized pattern stream

    ↓
    
LLM reads compressed file, spots the line of interest

    ↓

Compressed file allows referencing back to the original

    ↓ 
    
LLM reads a narrow window

    ↓ 
    
Root cause found without searching the the whole haystack
```

Each output line is a recurring pattern with volatile values replaced by
`~`-prefixed placeholders (`~TS`, `~UUID`, `~IP`, `~NUM`, etc.); a run of the same
placeholder collapses to one. Repeated patterns are prefixed with a count (`5x`),
so the LLM sees structure rather than noise.

## MCP tools

### `compress_log_file`
Reads a log file from disk, compresses it to a temp file, and writes a
line-number index (sidecar).

```json
{ "filePath": "/var/log/app.log" }
```

Returns `{ compressedPath, sidecarPath, summary }`. The LLM reads `compressedPath`
with its own file-reading tool.

### `get_original_lines`
Looks up which original line numbers match a pattern from the compressed output.

```json
{ "sidecarPath": "/tmp/loglens/abc123.json", "pattern": "ERROR ~TS connection refused to ~IP" }
```

Returns `{ pattern, count, lineNumbers, truncated, samples }` so the LLM can read
only those lines.

### `get_token_savings`
Reports cumulative savings across all `compress_log_file` calls.

Returns `{ compressions, rawBytes, compressedBytes, savedBytes, estTokensSaved }`.
`estTokensSaved` is a byte-based estimate (~4 bytes/token), not an exact tokenizer
count. Counters persist in `$TMPDIR/loglens/stats.json` and reset only when the OS
clears its temp directory.

### Compression

The server normalizes: timestamps, UUIDs, IPs,
hashes, paths, durations, and numbers

Identical patterns are deduplicated with a count prefix.

## Use it

LogLens runs as a stdio MCP server: your client (e.g. Claude Code) launches the
process and shuts it down for you.

Build the binary:

```sh
cd rust
cargo build --release      # produces target/release/loglens-mcp
```

Add to `.claude/settings.json` in your project (or `~/.claude/settings.json`):

```json
{
  "mcpServers": {
    "loglens": {
      "command": "/path/to/llm-lc/rust/target/release/loglens-mcp"
    }
  }
}
```

- **Start:** add the `mcpServers` block above, then restart your client (or run
  `/mcp` in Claude Code) to confirm `loglens` is connected.
- **Stop:** remove or disable the `loglens` entry and reload — the client
  terminates the spawned process.

## Performance

The scanner compressed at roughly **128 MB/s on 8 cores** (~1M-line /
176 MB sample), so even large log files
compress in well under a second.

It compresses [this Apache log](https://github.com/logpai/loghub/tree/master/Apache) from LogHub from 56,482 lines to 995, **reducing token usage by an estimated 99.98%**.