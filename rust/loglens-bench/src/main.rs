//! loglens-scan — benchmark + correctness CLI over `loglens-core`.
//!
//! Usage:
//!   loglens-scan <file> [--threads N] [--emit]
//!
//! Without --emit: prints a benchmark (scan-only 1-thread + rayon, aggregate
//!                 1-thread + rayon, group count).
//! With --emit:    prints {compressed, sidecar, summary} JSON to stdout (benchmark
//!                 lines go to stderr), for the JS-vs-Rust correctness diff.

use rayon::prelude::*;
use std::time::Instant;

use loglens_core::{
    aggregate, aggregate_parallel, build_pool, compress_bytes, emit_json, scan_line,
};

/// Time the full compress (aggregate + emit) — exactly what the MCP server runs —
/// single-thread then rayon. Output format is parsed by the matrix harness.
fn compress_pass(data: &[u8], threads: usize, nthreads: usize, mb: f64) {
    {
        let t = Instant::now();
        let o = compress_bytes(data, 1);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "  compress 1t :  {:>6.0} ms   {:>6.1} MB/s   groups {}",
            ms, mb / (ms / 1000.0), o.unique_groups
        );
    }
    {
        let t = Instant::now();
        let o = compress_bytes(data, threads);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "  compress r({}): {:>6.0} ms   {:>6.1} MB/s   groups {}",
            nthreads, ms, mb / (ms / 1000.0), o.unique_groups
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut file: Option<String> = None;
    let mut threads: usize = 0; // 0 = rayon default (all cores)
    let mut emit = false;
    let mut compress_only = false;
    let mut k = 1;
    while k < args.len() {
        match args[k].as_str() {
            "--threads" => { k += 1; threads = args.get(k).and_then(|v| v.parse().ok()).unwrap_or(0); }
            "--emit" => emit = true,
            "--compress-only" => compress_only = true,
            other => file = Some(other.to_string()),
        }
        k += 1;
    }
    let path = file.unwrap_or_else(|| {
        eprintln!("usage: loglens-scan <file> [--threads N] [--emit] [--compress-only]");
        std::process::exit(2);
    });

    let data = std::fs::read(&path).unwrap_or_else(|e| {
        eprintln!("read {}: {}", path, e);
        std::process::exit(1);
    });
    let lines: Vec<&[u8]> = data.split(|&b| b == b'\n').collect();
    let total_bytes = data.len();
    let mb = total_bytes as f64 / 1024.0 / 1024.0;

    if emit {
        let agg = if threads == 1 {
            aggregate(&lines)
        } else {
            aggregate_parallel(&lines, &build_pool(threads))
        };
        println!("{}", emit_json(&agg, lines.len()));
        return;
    }

    if compress_only {
        let nthreads = build_pool(threads).current_num_threads();
        eprintln!(
            "loglens-scan  {} lines ({:.1} MB)  conservative (compress-only)",
            lines.len(), mb
        );
        compress_pass(&data, threads, nthreads, mb);
        return;
    }

    eprintln!(
        "loglens-scan  {} lines ({:.1} MB)  conservative",
        lines.len(), mb
    );

    // scan-only, single thread (comparable to JS `micro`)
    {
        let t = Instant::now();
        let mut buf = Vec::new();
        let mut sink: usize = 0;
        for line in &lines {
            scan_line(line, &mut buf);
            sink += buf.len();
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "  scan 1-thread:  {:>6.0} ms   {:>6.1} MB/s   (sink {})",
            ms, mb / (ms / 1000.0), sink
        );
    }

    let pool = build_pool(threads);
    let nthreads = pool.current_num_threads();

    // scan-only, rayon (chunked so the output buffer is reused within a chunk)
    {
        let t = Instant::now();
        let sink: usize = pool.install(|| {
            lines
                .par_chunks(8192)
                .map(|chunk| {
                    let mut buf = Vec::new();
                    let mut s = 0usize;
                    for line in chunk {
                        scan_line(line, &mut buf);
                        s += buf.len();
                    }
                    s
                })
                .sum()
        });
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "  scan rayon({}):  {:>6.0} ms   {:>6.1} MB/s   (sink {})",
            nthreads, ms, mb / (ms / 1000.0), sink
        );
    }

    // aggregation pass (single-thread) — the realistic path that produces groups
    {
        let t = Instant::now();
        let agg = aggregate(&lines);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "  aggregate 1t :  {:>6.0} ms   {:>6.1} MB/s   groups {}",
            ms, mb / (ms / 1000.0), agg.group_count()
        );
    }

    // aggregation pass (rayon: thread-local maps + ordered merge)
    {
        let t = Instant::now();
        let agg = aggregate_parallel(&lines, &pool);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "  aggregate r({}): {:>6.0} ms   {:>6.1} MB/s   groups {}",
            nthreads, ms, mb / (ms / 1000.0), agg.group_count()
        );
    }

    // full compress (aggregate + emit) — exactly what the MCP server runs
    compress_pass(&data, threads, nthreads, mb);
}
