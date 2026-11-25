//! Bulk audits on Bend.
//!
//! `bend/audit.bend` checks thousands of visited hosts (browser history, DNS logs) against
//! the blocklist in parallel on HVM — on every CPU core with `run-c`, or on an NVIDIA GPU
//! with `run-cu`. Bend has no strings worth matching on, so Rust turns every name into
//! its labels' 24-bit hashes, reversed ("www.example.com" → [h(com), h(example), h(www)]);
//! a rule then matches a host exactly when it is a prefix of it.
//!
//! Hashes can collide, so every hit Bend reports is re-checked with the exact
//! [`DomainSet`] matcher; a collision can't block anything, and hashing never misses a
//! real match (equal labels always hash equal). Without Bend installed the identical
//! algorithm runs in Rust, so results never depend on the engine.

use crate::matcher::{normalize_host, DomainSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const KERNEL: &str = include_str!("../../../bend/audit.bend");

/// FNV-1a, folded to the 24 bits a Bend number holds.
pub fn label_hash(label: &str) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for b in label.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    (h >> 24) ^ (h & 0xff_ffff)
}

/// Reversed label hashes of a normalized host.
pub fn labels(host: &str) -> Vec<u32> {
    host.rsplit('.').map(label_hash).collect()
}

fn list(v: &[u32]) -> String {
    let items: Vec<String> = v.iter().map(u32::to_string).collect();
    format!("[{}]", items.join(", "))
}

/// HVM's C backend caps each definition's size, so the data is spread over many small
/// definitions: one per group of 24 hosts (and its subtrees), and rule chunks of `RULES`.
const GROUP: usize = 24;
const RULES: usize = 40;
/// Hosts per Bend run. HVM compiles each program to C first, and that cost grows faster
/// than the host count, so big audits run as several programs.
pub const BATCH: usize = 4096;

struct Defs {
    out: String,
    n: usize,
}

impl Defs {
    fn def(&mut self, body: String) -> String {
        let name = format!("d{}", self.n);
        self.n += 1;
        self.out.push_str(&format!("def {name}():\n  return {body}\n\n"));
        format!("{name}()")
    }

    /// Balanced tree over hosts of one group: leaves carry their bit.
    fn hosts(&mut self, items: &[(u32, Vec<u32>)]) -> String {
        match items {
            [] => "QTree/Leaf { bit: 0, labels: [] }".into(),
            [(bit, l)] => format!("QTree/Leaf {{ bit: {bit}, labels: {} }}", list(l)),
            _ => {
                let (a, b) = items.split_at(items.len() / 2);
                let e = format!("QTree/Node {{ left: {}, right: {} }}", self.hosts(a), self.hosts(b));
                if items.len() >= GROUP / 2 { self.def(e) } else { e }
            }
        }
    }

    /// Balanced tree of groups.
    fn groups(&mut self, groups: &[(usize, String)]) -> String {
        match groups {
            [] => "QTree/Leaf { bit: 0, labels: [] }".into(),
            [(gid, hosts)] => format!("QTree/Group {{ gid: {gid}, hosts: {hosts} }}"),
            _ => {
                let (a, b) = groups.split_at(groups.len() / 2);
                let e = format!("QTree/Node {{ left: {}, right: {} }}", self.groups(a), self.groups(b));
                if groups.len() >= 8 { self.def(e) } else { e }
            }
        }
    }
}

/// The complete Bend program for one audit batch.
pub fn program(rules: &[String], hosts: &[String]) -> String {
    let rules: Vec<String> = rules.iter().filter_map(|r| normalize_host(r)).map(|r| list(&labels(&r))).collect();
    let mut d = Defs { out: String::new(), n: 0 };
    // Rules as a chain of chunks, built back to front: rK() = concat([...], rK+1()).
    let mut tail = "[]".to_string();
    for chunk in rules.chunks(RULES).rev() {
        tail = d.def(format!("concat([{}], {tail})", chunk.join(", ")));
    }
    let groups: Vec<(usize, String)> = hosts
        .chunks(GROUP)
        .enumerate()
        .map(|(g, chunk)| {
            let items: Vec<(u32, Vec<u32>)> = chunk.iter().enumerate().map(|(i, h)| (1u32 << i, labels(h))).collect();
            (g, d.hosts(&items))
        })
        .collect();
    let tree = d.groups(&groups);
    let mut s = String::from(KERNEL);
    s.push('\n');
    s.push_str(&d.out);
    s.push_str(&format!("def main():\n  rules = {tail}\n  return audit({tree}, rules)\n"));
    s
}

/// Turns Bend's [group, mask, group, mask, ...] into host indices.
pub fn unpack(pairs: &[usize]) -> Vec<usize> {
    let mut out = Vec::new();
    for p in pairs.chunks(2) {
        if let [g, mask] = *p {
            out.extend((0..GROUP).filter(|i| mask & (1 << i) != 0).map(|i| g * GROUP + i));
        }
    }
    out
}

/// Reads `Result: [3, 7]` from Bend's output.
pub fn parse_output(out: &str) -> Option<Vec<usize>> {
    let line = out.lines().rev().find(|l| l.trim_start().starts_with("Result:"))?;
    let inner = line.split_once('[')?.1.rsplit_once(']')?.0;
    inner.split(',').map(str::trim).filter(|s| !s.is_empty()).map(|s| s.parse().ok()).collect()
}

/// The same algorithm as the Bend kernel, for machines without Bend and for parity tests.
pub fn audit_hashed(rules: &[String], hosts: &[String]) -> Vec<usize> {
    let rules: Vec<Vec<u32>> = rules.iter().filter_map(|r| normalize_host(r)).map(|r| labels(&r)).collect();
    hosts
        .iter()
        .enumerate()
        .filter(|(_, h)| {
            let l = labels(h);
            rules.iter().any(|r| r.len() <= l.len() && r[..] == l[..r.len()])
        })
        .map(|(i, _)| i)
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// HVM on all CPU cores.
    BendC,
    /// HVM on an NVIDIA GPU.
    BendCuda,
    Rust,
}

impl Engine {
    pub fn name(self) -> &'static str {
        match self {
            Engine::BendC => "Bend (HVM, all cores)",
            Engine::BendCuda => "Bend (HVM, GPU)",
            Engine::Rust => "Rust",
        }
    }
}

/// Finds `bend` (and the `hvm` it drives) on PATH or in ~/.cargo/bin.
pub fn find_bend() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "bend.exe" } else { "bend" };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(h) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        dirs.push(Path::new(&h).join(".cargo").join("bin"));
    }
    dirs.into_iter().map(|d| d.join(exe)).find(|p| p.is_file())
}

pub struct Audit {
    pub engine: Engine,
    /// (host, rule that blocks it)
    pub blocked: Vec<(String, String)>,
    pub checked: usize,
    /// Hash collisions Bend reported that the exact check threw out.
    pub false_hits: usize,
}

fn run_bend(bend: &Path, cuda: bool, src: &str) -> Result<Vec<usize>, String> {
    let dir = std::env::temp_dir().join(format!("thurea-audit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join("audit.bend");
    std::fs::File::create(&file).and_then(|mut f| f.write_all(src.as_bytes())).map_err(|e| e.to_string())?;
    // bend calls `hvm`; make sure the directory bend lives in is searched too.
    let mut path = bend.parent().map(|p| vec![p.to_path_buf()]).unwrap_or_default();
    if let Some(p) = std::env::var_os("PATH") {
        path.extend(std::env::split_paths(&p));
    }
    let out = Command::new(bend)
        .arg(if cuda { "run-cu" } else { "run-c" })
        .arg(&file)
        .env("PATH", std::env::join_paths(path).map_err(|e| e.to_string())?)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string());
    let _ = std::fs::remove_dir_all(&dir);
    let out = out?;
    let text = String::from_utf8_lossy(&out.stdout);
    parse_output(&text).map(|p| unpack(&p)).ok_or_else(|| format!("bend: {}{}", text.trim(), String::from_utf8_lossy(&out.stderr).trim()))
}

/// Audits hosts against rules. `prefer` picks the engine; Bend falls back to Rust if it is
/// missing or fails, and the report says which engine actually ran.
pub fn audit(rules: &[String], inputs: &[String], prefer: Engine) -> Audit {
    let hosts: Vec<String> = inputs.iter().filter_map(|h| normalize_host(h)).collect();
    let exact = DomainSet::new(rules);
    let (engine, hits) = match (prefer, find_bend()) {
        (Engine::BendC | Engine::BendCuda, Some(b)) if !hosts.is_empty() => {
            let mut all = Vec::new();
            let mut failed = false;
            for (n, batch) in hosts.chunks(BATCH).enumerate() {
                match run_bend(&b, prefer == Engine::BendCuda, &program(rules, batch)) {
                    Ok(h) => all.extend(h.into_iter().map(|i| i + n * BATCH)),
                    Err(_) => {
                        failed = true;
                        break;
                    }
                }
            }
            if failed {
                (Engine::Rust, audit_hashed(rules, &hosts))
            } else {
                (prefer, all)
            }
        }
        _ => (Engine::Rust, audit_hashed(rules, &hosts)),
    };
    let mut blocked = Vec::new();
    let mut false_hits = 0;
    for i in hits {
        match hosts.get(i).and_then(|h| exact.matching_rule(h).map(|r| (h.clone(), r.to_string()))) {
            Some(hit) => blocked.push(hit),
            None => false_hits += 1,
        }
    }
    Audit { engine, blocked, checked: hosts.len(), false_hits }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Vec<String> {
        vec!["pornhub.com".into(), "rule34.xxx".into(), "example.org".into()]
    }

    fn hosts() -> Vec<String> {
        ["www.pornhub.com", "news.bbc.co.uk", "rule34.xxx", "pornhub.com.evil.net", "a.b.example.org", "org"]
            .map(String::from)
            .to_vec()
    }

    #[test]
    fn hashed_algorithm_matches_exact_matcher() {
        assert_eq!(audit_hashed(&rules(), &hosts()), vec![0, 2, 4]);
        let set = DomainSet::new(rules());
        for (i, h) in hosts().iter().enumerate() {
            assert_eq!(set.is_blocked(h), [0, 2, 4].contains(&i), "{h}");
        }
    }

    #[test]
    fn hashes_fit_bend_numbers() {
        for l in ["com", "pornhub", "", "x".repeat(63).as_str()] {
            assert!(label_hash(l) < 1 << 24);
        }
    }

    #[test]
    fn parses_bend_output() {
        assert_eq!(parse_output("Result: [0, 2, 4]\n"), Some(vec![0, 2, 4]));
        assert_eq!(parse_output("warn\nResult: []"), Some(vec![]));
        assert_eq!(parse_output("Errors: boom"), None);
    }

    #[test]
    fn unpacks_masks() {
        assert_eq!(unpack(&[0, 0b101, 2, 1]), vec![0, 2, 48]);
    }

    #[test]
    fn program_contains_kernel_and_main() {
        let p = program(&rules(), &hosts());
        assert!(p.contains("def audit(tree, rules):"));
        assert!(p.contains("def main():"));
        assert!(p.contains("QTree/Group"));
    }

    /// Runs the real Bend kernel when Bend is installed; skipped otherwise.
    #[test]
    fn bend_and_rust_agree() {
        if find_bend().is_none() {
            eprintln!("bend not installed: skipping");
            return;
        }
        let mut many = hosts();
        for i in 0..300 {
            many.push(format!("site{i}.example.net"));
            many.push(format!("cdn{i}.pornhub.com"));
        }
        let p = crate::Policy::default();
        let a = audit(&p.blocklist, &many, Engine::BendC);
        assert_eq!(a.engine, Engine::BendC, "Bend did not run");
        let r = audit(&p.blocklist, &many, Engine::Rust);
        assert_eq!(a.blocked, r.blocked);
        assert_eq!(a.blocked.len(), 302);
    }
}
