//! loglens-core — single-pass log normalization scanner + aggregation.
//!
//! Conservative tier: a left-to-right byte scanner replaces volatile tokens with
//! `~`-prefixed placeholders (`~TS`, `~IP`, `~NUM`, …) and collapses a run of the
//! same placeholder separated only by whitespace or `, ; : _`. Operates on raw
//! bytes; recognizers only ever match ASCII tokens, so non-ASCII (UTF-8
//! continuation) bytes are copied through like any other literal.
//!
//! `compress_bytes`/`compress_path` are the high-level entrypoints the Rust MCP
//! server and the bench bin call; the lower-level `scan_line`/`aggregate*`/
//! `emit_*` functions stay public for the bench's per-pass timings.

use ahash::AHashMap;
use rayon::prelude::*;
use std::path::Path;

// ── char-class predicates (byte-level; -1 sentinel == JS charCodeAt(NaN)) ───────
#[inline]
fn at(s: &[u8], p: usize) -> i32 {
    if p < s.len() { s[p] as i32 } else { -1 }
}
#[inline]
fn is_space(c: i32) -> bool {
    c == 32 || c == 9 || c == 10 || c == 13 || c == 12 || c == 11
}
#[inline]
fn is_digit(c: i32) -> bool { c >= 48 && c <= 57 }
#[inline]
fn is_lower(c: i32) -> bool { c >= 97 && c <= 122 }
#[inline]
fn is_upper(c: i32) -> bool { c >= 65 && c <= 90 }
#[inline]
fn is_alpha(c: i32) -> bool { is_lower(c) || is_upper(c) }
#[inline]
fn is_alnum(c: i32) -> bool { is_digit(c) || is_alpha(c) }
#[inline]
fn is_word(c: i32) -> bool { is_alnum(c) || c == 95 }
#[inline]
fn is_hex(c: i32) -> bool { is_digit(c) || (c >= 97 && c <= 102) || (c >= 65 && c <= 70) }
#[inline]
fn is_path_char(c: i32) -> bool { is_word(c) || c == 46 || c == 45 }
/// Separators across which a run of the same placeholder is merged: whitespace,
/// or one of `,` `;` `:` `_`.
#[inline]
fn is_merge_sep(c: i32) -> bool {
    is_space(c) || c == 44 || c == 59 || c == 58 || c == 95
}

#[inline]
fn d2(s: &[u8], p: usize, n: usize) -> bool {
    p + 1 < n && is_digit(at(s, p)) && is_digit(at(s, p + 1))
}
#[inline]
fn end_boundary(s: &[u8], p: usize, n: usize) -> bool {
    p >= n || !is_word(at(s, p))
}
#[inline]
fn literal_at(s: &[u8], p: usize, n: usize, lit: &[u8]) -> bool {
    if p + lit.len() > n { return false; }
    for k in 0..lit.len() {
        if s[p + k] != lit[k] { return false; }
    }
    true
}
#[inline]
fn octet(s: &[u8], p: usize, n: usize) -> usize {
    if p >= n || !is_digit(at(s, p)) { return 0; }
    let mut len = 1;
    if p + 1 < n && is_digit(at(s, p + 1)) {
        len = 2;
        if p + 2 < n && is_digit(at(s, p + 2)) { len = 3; }
    }
    len
}
#[inline]
fn hex_run(s: &[u8], p: usize, n: usize) -> usize {
    let mut q = p;
    while q < n && is_hex(at(s, q)) { q += 1; }
    q - p
}

// ── recognizers (return match length at i; 0 = no match) ────────────────────────

fn m_ansi(s: &[u8], i: usize, n: usize) -> usize {
    if at(s, i) != 27 || at(s, i + 1) != 91 { return 0; }
    let mut p = i + 2;
    while p < n {
        let c = at(s, p);
        if c == 109 { return p - i + 1; }
        if !is_digit(c) && c != 59 { return 0; }
        p += 1;
    }
    0
}

fn m_ts(s: &[u8], i: usize, n: usize) -> usize {
    let mut p = i;
    for _ in 0..4 { if p >= n || !is_digit(at(s, p)) { return 0; } p += 1; }
    if at(s, p) != 45 { return 0; } p += 1;
    if !d2(s, p, n) { return 0; } p += 2;
    if at(s, p) != 45 { return 0; } p += 1;
    if !d2(s, p, n) { return 0; } p += 2;
    let sep = at(s, p); p += 1;
    if sep != 84 && sep != 32 { return 0; }
    if !d2(s, p, n) { return 0; } p += 2;
    if at(s, p) != 58 { return 0; } p += 1;
    if !d2(s, p, n) { return 0; } p += 2;
    if at(s, p) != 58 { return 0; } p += 1;
    if !d2(s, p, n) { return 0; } p += 2;
    if at(s, p) == 46 && p + 1 < n && is_digit(at(s, p + 1)) {
        p += 1;
        while p < n && is_digit(at(s, p)) { p += 1; }
    }
    let z = at(s, p);
    if z == 90 {
        p += 1;
    } else if z == 43 || z == 45 {
        if d2(s, p + 1, n) && at(s, p + 3) == 58 && d2(s, p + 4, n) { p += 6; }
    }
    p - i
}

fn m_date(s: &[u8], i: usize, n: usize) -> usize {
    // YYYY-MM-DD
    if d2(s, i, n) && d2(s, i + 2, n) && at(s, i + 4) == 45
        && d2(s, i + 5, n) && at(s, i + 7) == 45 && d2(s, i + 8, n)
    {
        return 10;
    }
    // MM/DD/YYYY
    if d2(s, i, n) && at(s, i + 2) == 47 && d2(s, i + 3, n)
        && at(s, i + 5) == 47 && d2(s, i + 6, n) && d2(s, i + 8, n)
    {
        return 10;
    }
    0
}

fn m_time(s: &[u8], i: usize, n: usize) -> usize {
    if !(d2(s, i, n) && at(s, i + 2) == 58 && d2(s, i + 3, n)
        && at(s, i + 5) == 58 && d2(s, i + 6, n)) { return 0; }
    let mut p = i + 8;
    if at(s, p) == 46 && p + 1 < n && is_digit(at(s, p + 1)) {
        p += 1;
        while p < n && is_digit(at(s, p)) { p += 1; }
    }
    p - i
}

fn m_url(s: &[u8], i: usize, n: usize) -> usize {
    let mut p = i;
    if !literal_at(s, p, n, b"http") { return 0; }
    p += 4;
    if at(s, p) == 115 { p += 1; }
    if !literal_at(s, p, n, b"://") { return 0; }
    p += 3;
    let start = p;
    while p < n && !is_space(at(s, p)) { p += 1; }
    if p == start { return 0; }
    p - i
}

fn m_email(s: &[u8], i: usize, n: usize) -> usize {
    if is_space(at(s, i)) { return 0; }
    let mut end = i;
    while end < n && !is_space(at(s, end)) { end += 1; }
    let mut atpos: i64 = -1;
    let mut p = i + 1;
    while p < end {
        if at(s, p) == 64 { atpos = p as i64; break; }
        p += 1;
    }
    if atpos < 0 || (atpos as usize) + 2 >= end { return 0; }
    let a = atpos as usize;
    let mut q = a + 2;
    while q <= end - 2 {
        if at(s, q) == 46 { return end - i; }
        q += 1;
    }
    0
}

fn m_path(s: &[u8], i: usize, n: usize) -> usize {
    if at(s, i) == 47 {
        let mut p = i;
        let mut segs = 0;
        while p < n && at(s, p) == 47 {
            let mut q = p + 1;
            if q >= n || !is_path_char(at(s, q)) { break; }
            while q < n && is_path_char(at(s, q)) { q += 1; }
            p = q;
            segs += 1;
        }
        if segs >= 2 { return p - i; }
        return 0;
    }
    if is_alpha(at(s, i)) && at(s, i + 1) == 58 && at(s, i + 2) == 92 {
        let mut p = i + 3;
        let start = p;
        while p < n && !is_space(at(s, p)) { p += 1; }
        if p == start { return 0; }
        return p - i;
    }
    0
}

fn m_uuid(s: &[u8], i: usize, n: usize) -> usize {
    let mut p = i;
    for _ in 0..8 { if p >= n || !is_hex(at(s, p)) { return 0; } p += 1; }
    if at(s, p) != 45 { return 0; } p += 1;
    for _ in 0..4 { if p >= n || !is_hex(at(s, p)) { return 0; } p += 1; }
    if at(s, p) != 45 { return 0; } p += 1;
    for _ in 0..4 { if p >= n || !is_hex(at(s, p)) { return 0; } p += 1; }
    if at(s, p) != 45 { return 0; } p += 1;
    for _ in 0..4 { if p >= n || !is_hex(at(s, p)) { return 0; } p += 1; }
    if at(s, p) != 45 { return 0; } p += 1;
    for _ in 0..12 { if p >= n || !is_hex(at(s, p)) { return 0; } p += 1; }
    p - i
}

fn m_hash(s: &[u8], i: usize, n: usize) -> usize {
    let len = hex_run(s, i, n);
    if len >= 16 && end_boundary(s, i + len, n) { return len; }
    0
}

fn m_hex(s: &[u8], i: usize, n: usize) -> usize {
    if at(s, i) != 48 || at(s, i + 1) != 120 { return 0; }
    let len = hex_run(s, i + 2, n);
    if len == 0 { return 0; }
    2 + len
}

fn m_mac(s: &[u8], i: usize, _n: usize) -> usize {
    let mut p = i;
    for _ in 0..5 {
        if !is_hex(at(s, p)) || !is_hex(at(s, p + 1)) || at(s, p + 2) != 58 { return 0; }
        p += 3;
    }
    if !is_hex(at(s, p)) || !is_hex(at(s, p + 1)) { return 0; }
    p + 2 - i
}

fn m_ip(s: &[u8], i: usize, n: usize) -> usize {
    let mut p = i;
    let mut o = octet(s, p, n);
    if o == 0 { return 0; }
    p += o;
    for _ in 0..3 {
        if at(s, p) != 46 { return 0; }
        o = octet(s, p + 1, n);
        if o == 0 { return 0; }
        p += 1 + o;
    }
    if !end_boundary(s, p, n) { return 0; }
    if at(s, p) == 58 && p + 1 < n && is_digit(at(s, p + 1)) {
        p += 1;
        while p < n && is_digit(at(s, p)) { p += 1; }
    }
    p - i
}

const DUR_UNITS: [&[u8]; 6] = [b"ns", b"us", b"ms", b"s", b"m", b"h"];
fn m_dur(s: &[u8], i: usize, n: usize) -> usize {
    if !is_digit(at(s, i)) { return 0; }
    let mut p = i + 1;
    while p < n && is_digit(at(s, p)) { p += 1; }
    if at(s, p) == 46 && p + 1 < n && is_digit(at(s, p + 1)) {
        p += 1;
        while p < n && is_digit(at(s, p)) { p += 1; }
    }
    for u in DUR_UNITS {
        if literal_at(s, p, n, u) && end_boundary(s, p + u.len(), n) {
            return p + u.len() - i;
        }
    }
    0
}

const SIZE_UNITS: [&[u8]; 8] = [b"B", b"KB", b"MB", b"GB", b"TB", b"KiB", b"MiB", b"GiB"];
fn m_size(s: &[u8], i: usize, n: usize) -> usize {
    if !is_digit(at(s, i)) { return 0; }
    let mut p = i + 1;
    while p < n && is_digit(at(s, p)) { p += 1; }
    if at(s, p) == 46 && p + 1 < n && is_digit(at(s, p + 1)) {
        p += 1;
        while p < n && is_digit(at(s, p)) { p += 1; }
    }
    if p < n && is_space(at(s, p)) { p += 1; }
    for u in SIZE_UNITS {
        if literal_at(s, p, n, u) && end_boundary(s, p + u.len(), n) {
            return p + u.len() - i;
        }
    }
    0
}

fn m_num(s: &[u8], i: usize, n: usize) -> usize {
    if !is_digit(at(s, i)) { return 0; }
    let mut p = i + 1;
    while p < n && is_digit(at(s, p)) { p += 1; }
    if at(s, p) == 46 && p + 1 < n && is_digit(at(s, p + 1)) {
        let after = p + 1;
        if !(m_date(s, after, n) > 0 || m_ts(s, after, n) > 0 || m_time(s, after, n) > 0) {
            p += 1;
            while p < n && is_digit(at(s, p)) { p += 1; }
        }
    }
    p - i
}

/// First-char dispatch in rule-priority order. Returns (match length, placeholder).
#[inline]
fn match_at(s: &[u8], i: usize, n: usize, bb: bool, run_start: bool) -> Option<(usize, &'static [u8])> {
    let c = at(s, i);
    let digit = c >= 48 && c <= 57;
    let hex = is_hex(c);
    if c == 27 { let l = m_ansi(s, i, n); if l > 0 { return Some((l, b"")); } }
    if digit { let l = m_ts(s, i, n); if l > 0 { return Some((l, b"~TS")); } }
    if digit { let l = m_date(s, i, n); if l > 0 { return Some((l, b"~DATE")); } }
    if digit { let l = m_time(s, i, n); if l > 0 { return Some((l, b"~TIME")); } }
    if c == 104 { let l = m_url(s, i, n); if l > 0 { return Some((l, b"~URL")); } }
    if run_start && !is_space(c) { let l = m_email(s, i, n); if l > 0 { return Some((l, b"~EMAIL")); } }
    if c == 47 || is_alpha(c) { let l = m_path(s, i, n); if l > 0 { return Some((l, b"~PATH")); } }
    if hex { let l = m_uuid(s, i, n); if l > 0 { return Some((l, b"~UUID")); } }
    if bb && hex { let l = m_hash(s, i, n); if l > 0 { return Some((l, b"~HASH")); } }
    if c == 48 { let l = m_hex(s, i, n); if l > 0 { return Some((l, b"~HEX")); } }
    if hex { let l = m_mac(s, i, n); if l > 0 { return Some((l, b"~MAC")); } }
    if bb && digit { let l = m_ip(s, i, n); if l > 0 { return Some((l, b"~IP")); } }
    if bb && digit { let l = m_dur(s, i, n); if l > 0 { return Some((l, b"~DUR")); } }
    if bb && digit { let l = m_size(s, i, n); if l > 0 { return Some((l, b"~SIZE")); } }
    if digit { let l = m_num(s, i, n); if l > 0 { return Some((l, b"~NUM")); } }
    None
}

/// Normalize one line (conservative tier) into `out`.
pub fn scan_line(s: &[u8], out: &mut Vec<u8>) {
    out.clear();
    let n = s.len();
    let mut i = 0;
    let mut mark = 0;
    // Replacement bytes of the last placeholder emitted, for collapsing a run of
    // the same placeholder separated only by merge-separators.
    let mut last_ph: Option<&'static [u8]> = None;
    while i < n {
        let c = s[i] as i32;
        // Fast path: a space (the most common byte) can never start a token, so
        // leave it in the pending literal run without computing boundaries.
        if c == 32 {
            i += 1;
            continue;
        }
        let prev: i32 = if i == 0 { -1 } else { s[i - 1] as i32 };
        let bb = prev == -1 || !is_word(prev);
        let run_start = prev == -1 || is_space(prev);
        // Mid-run punctuation (not alnum, not '/', not ESC) starts no recognizer
        // and EMAIL only fires at a run start — skip match_at entirely.
        if !run_start && !is_alnum(c) && c != 27 && c != 47 {
            i += 1;
            continue;
        }
        if let Some((len, rep)) = match_at(s, i, n, bb, run_start) {
            let gap = &s[mark..i];
            if !rep.is_empty()
                && last_ph == Some(rep)
                && gap.iter().all(|&b| is_merge_sep(b as i32))
            {
                // Same placeholder as the previous, separated only by merge-
                // separators (or nothing): drop this duplicate and the gap.
                i += len;
                mark = i;
            } else {
                if mark < i { out.extend_from_slice(gap); }
                out.extend_from_slice(rep);
                i += len;
                mark = i;
                last_ph = if rep.is_empty() { None } else { Some(rep) };
            }
        } else {
            i += 1;
        }
    }
    if mark < n { out.extend_from_slice(&s[mark..n]); }
}

// ── aggregation (mirrors SidecarEntry) ──────────────────────────────────────────
/// How many original line numbers to retain per pattern in the sidecar.
pub const MAX_LINE_NUMBERS: usize = 50;
/// How many raw sample lines to retain per pattern.
pub const MAX_SAMPLES: usize = 5;

struct Entry {
    count: u64,
    line_numbers: Vec<u32>,
    samples: Vec<Vec<u8>>,
}

/// Aggregated patterns: pattern→entry map plus first-occurrence order.
pub struct Agg {
    map: AHashMap<Vec<u8>, Entry>,
    order: Vec<Vec<u8>>,
}

impl Agg {
    /// Number of unique patterns (compressed groups).
    pub fn group_count(&self) -> usize {
        self.order.len()
    }
}

/// Fold one contiguous slice of lines into a partial Agg, writing **absolute**
/// line numbers (caller passes the slice's 1-based starting line number).
fn aggregate_chunk(lines: &[&[u8]], start_lineno: u32) -> Agg {
    let mut map: AHashMap<Vec<u8>, Entry> = AHashMap::with_capacity(256);
    let mut order: Vec<Vec<u8>> = Vec::new();
    let mut buf = Vec::new();
    let mut lineno = start_lineno;
    for line in lines {
        scan_line(line, &mut buf);
        match map.get_mut(buf.as_slice()) {
            Some(e) => {
                e.count += 1;
                if e.line_numbers.len() < MAX_LINE_NUMBERS { e.line_numbers.push(lineno); }
                if e.samples.len() < MAX_SAMPLES { e.samples.push(line.to_vec()); }
            }
            None => {
                let key = buf.clone();
                order.push(key.clone());
                map.insert(key, Entry {
                    count: 1,
                    line_numbers: vec![lineno],
                    samples: vec![line.to_vec()],
                });
            }
        }
        lineno += 1;
    }
    Agg { map, order }
}

/// Single-thread aggregation over the whole input (lines numbered from 1).
pub fn aggregate(lines: &[&[u8]]) -> Agg {
    aggregate_chunk(lines, 1)
}

/// Merge a partial Agg into `global`, preserving first-occurrence order and the
/// per-pattern caps. Call with partials in ascending source order so retained
/// line numbers and samples stay sorted (mirrors `mergePartial` in artifacts.ts).
fn merge_into(global: &mut Agg, part: Agg) {
    let Agg { mut map, order } = part;
    for key in order {
        let pe = map.remove(&key).expect("order key present in map");
        match global.map.get_mut(&key) {
            Some(e) => {
                e.count += pe.count;
                for ln in pe.line_numbers {
                    if e.line_numbers.len() >= MAX_LINE_NUMBERS { break; }
                    e.line_numbers.push(ln);
                }
                for sm in pe.samples {
                    if e.samples.len() >= MAX_SAMPLES { break; }
                    e.samples.push(sm);
                }
            }
            None => {
                global.order.push(key.clone());
                global.map.insert(key, pe);
            }
        }
    }
}

/// Parallel aggregation: scan line-chunks into thread-local Aggs (rayon), then
/// merge them in chunk order. Output is byte-identical to `aggregate`.
pub fn aggregate_parallel(lines: &[&[u8]], pool: &rayon::ThreadPool) -> Agg {
    let nthreads = pool.current_num_threads().max(1);
    let chunk = (lines.len() / (nthreads * 4)).max(1024);
    let parts: Vec<Agg> = pool.install(|| {
        lines
            .par_chunks(chunk)
            .enumerate()
            .map(|(idx, ch)| aggregate_chunk(ch, (idx * chunk) as u32 + 1))
            .collect()
    });
    let mut global = Agg { map: AHashMap::with_capacity(1024), order: Vec::new() };
    for part in parts {
        merge_into(&mut global, part);
    }
    global
}

// ── JSON emit (hand-rolled; only dep is rayon/ahash) ────────────────────────────
fn json_escape(bytes: &[u8], out: &mut String) {
    out.push('"');
    let s = String::from_utf8_lossy(bytes);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The `compressed` text: one pattern per line, `Nx`-prefixed when repeated,
/// in first-occurrence order. Byte-identical to JS `scanEmit`.
pub fn emit_compressed(agg: &Agg) -> String {
    let mut compressed = String::new();
    for (idx, key) in agg.order.iter().enumerate() {
        let e = &agg.map[key];
        let pat = String::from_utf8_lossy(key);
        if idx > 0 { compressed.push('\n'); }
        if e.count > 1 {
            compressed.push_str(&format!("{}x {}", e.count, pat));
        } else {
            compressed.push_str(&pat);
        }
    }
    compressed
}

/// The pattern→entry sidecar object (a JSON object string).
pub fn emit_sidecar_json(agg: &Agg) -> String {
    let mut sidecar = String::from("{");
    let mut first = true;
    for key in agg.order.iter() {
        let e = &agg.map[key];
        if !first { sidecar.push(','); }
        first = false;
        sidecar.push('\n');
        json_escape(key, &mut sidecar);
        sidecar.push(':');
        sidecar.push('{');
        sidecar.push_str("\"pattern\":");
        json_escape(key, &mut sidecar);
        sidecar.push_str(&format!(",\"count\":{},", e.count));
        sidecar.push_str("\"lineNumbers\":[");
        for (j, ln) in e.line_numbers.iter().enumerate() {
            if j > 0 { sidecar.push(','); }
            sidecar.push_str(&ln.to_string());
        }
        sidecar.push_str("],\"samples\":[");
        for (j, sm) in e.samples.iter().enumerate() {
            if j > 0 { sidecar.push(','); }
            json_escape(sm, &mut sidecar);
        }
        sidecar.push_str("]}");
    }
    sidecar.push_str("\n}");
    sidecar
}

/// `{compressed, sidecar, summary}` root object — used by the bench `--emit`
/// path and the JS-vs-Rust correctness diff.
pub fn emit_json(agg: &Agg, total_lines: usize) -> String {
    let compressed = emit_compressed(agg);
    let sidecar = emit_sidecar_json(agg);
    let groups = agg.order.len();
    let ratio = if total_lines == 0 { 0.0 } else { groups as f64 / total_lines as f64 };
    let mut root = String::from("{\"compressed\":");
    json_escape(compressed.as_bytes(), &mut root);
    root.push_str(",\"sidecar\":");
    root.push_str(&sidecar);
    root.push_str(&format!(
        ",\"summary\":{{\"totalLines\":{},\"uniqueGroups\":{},\"compressionRatio\":{}}}}}",
        total_lines, groups, ratio
    ));
    root
}

/// Build a rayon pool: `threads == 0` means rayon's default (all cores).
pub fn build_pool(threads: usize) -> rayon::ThreadPool {
    let mut b = rayon::ThreadPoolBuilder::new();
    if threads > 0 {
        b = b.num_threads(threads);
    }
    b.build().unwrap()
}

// ── high-level compression API (what the MCP server calls) ───────────────────────

/// Result of compressing a log: the rendered compressed text, the pattern→entry
/// sidecar (as a JSON object string), and summary stats.
pub struct CompressOutput {
    pub compressed: String,
    pub sidecar_json: String,
    /// Raw input size in bytes (for token-savings accounting).
    pub input_bytes: usize,
    pub total_lines: usize,
    pub unique_groups: usize,
    pub compression_ratio: f64,
}

/// Compress raw bytes. `threads == 1` runs single-threaded; anything else uses a
/// rayon pool (`0` = all cores). Lines are split on `\n` exactly like the JS side.
pub fn compress_bytes(data: &[u8], threads: usize) -> CompressOutput {
    let lines: Vec<&[u8]> = data.split(|&b| b == b'\n').collect();
    let total_lines = lines.len();
    let agg = if threads == 1 {
        aggregate(&lines)
    } else {
        aggregate_parallel(&lines, &build_pool(threads))
    };
    let unique_groups = agg.group_count();
    let compression_ratio = if total_lines == 0 {
        0.0
    } else {
        unique_groups as f64 / total_lines as f64
    };
    CompressOutput {
        compressed: emit_compressed(&agg),
        sidecar_json: emit_sidecar_json(&agg),
        input_bytes: data.len(),
        total_lines,
        unique_groups,
        compression_ratio,
    }
}

/// Read a file and compress it (see [`compress_bytes`]).
pub fn compress_path(path: &Path, threads: usize) -> std::io::Result<CompressOutput> {
    let data = std::fs::read(path)?;
    Ok(compress_bytes(&data, threads))
}

#[cfg(test)]
mod tests {
    use super::scan_line;

    fn norm(s: &str) -> String {
        let mut out = Vec::new();
        scan_line(s.as_bytes(), &mut out);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn placeholders_use_tilde_marker() {
        assert_eq!(norm("got 42"), "got ~NUM");
    }

    #[test]
    fn merges_run_across_whitespace() {
        assert_eq!(norm("1 2 3 ms"), "~NUM ms");
    }

    #[test]
    fn merges_run_across_separators() {
        // comma+space list and colon/semicolon separators collapse
        assert_eq!(norm("[1, 2, 3]"), "[~NUM]");
    }

    #[test]
    fn merges_run_across_underscores() {
        assert_eq!(norm("attempt_1445144423722_0020"), "attempt_~NUM");
    }

    #[test]
    fn does_not_merge_across_literal_text() {
        assert_eq!(norm("a=1; b=2"), "a=~NUM; b=~NUM");
    }

    #[test]
    fn does_not_merge_distinct_placeholders() {
        assert_eq!(norm("2024-01-02 foo 03:04:05"), "~DATE foo ~TIME");
    }

    #[test]
    fn leaves_plain_text_untouched() {
        assert_eq!(norm("hello world"), "hello world");
    }
}
