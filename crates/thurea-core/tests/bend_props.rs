//! Property tests run on Bend (HVM): the parallel runtime checks Rust's logic at a scale
//! that would be slow to spell out case by case.
//!
//! * `bedtime_windows_match_bend_spec`: `bend/tests/window.bend` is an independent model of
//!   `screen::Window`. For 5184 windows x 1440 minutes, Bend and Rust must agree.
//! * `audit_fuzz_bend_matches_exact_matcher`: thousands of random hosts, URLs and rules
//!   (with a real 24-bit hash collision planted) go through `thurea audit --engine bend`;
//!   every verdict must equal the exact Rust `DomainSet`, across several Bend batches.
//!
//! Both skip when Bend isn't installed (`cargo install hvm bend-lang`); set
//! `THUREA_REQUIRE_BEND=1` to make a missing Bend a failure instead.

use std::collections::HashMap;
use std::process::Command;
use thurea_core::bend::{self, Engine};
use thurea_core::matcher::{normalize_host, DomainSet};
use thurea_core::screen::Window;

fn bend_or_skip() -> Option<std::path::PathBuf> {
    let b = bend::find_bend();
    if b.is_none() {
        assert!(std::env::var_os("THUREA_REQUIRE_BEND").is_none(), "THUREA_REQUIRE_BEND is set but bend isn't installed");
        eprintln!("bend not installed: skipping");
    }
    b
}

/// Small deterministic PRNG (xorshift64*), so failures reproduce.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len())]
    }
}

const STEP: u16 = 20;

/// The same window list as window.bend.
fn window(i: u16, j: u16) -> Window {
    Window { start: (i * STEP + i % 3) % 1440, end: (j * STEP + j % 5) % 1440 }
}

/// Rust's side of window.bend's fingerprints, from the real `Window`.
fn rust_row(i: u16) -> [u32; 4] {
    let mut f = [0u32; 4];
    for j in 0..1440 / STEP {
        let w = window(i, j);
        for m in 0..1440u16 {
            let c = w.minutes_to_change(m) as u32;
            if w.contains(m) {
                f[0] += 1;
                f[1] += m as u32;
            }
            f[2] += c;
            f[3] ^= (c << 11) | m as u32;
        }
    }
    // Bend numbers are 24-bit and wrap.
    f.map(|x| x & 0xff_ffff)
}

fn run_bend(bend: &std::path::Path, src: &str, name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("thurea-bend-props-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{name}.bend"));
    std::fs::write(&file, src).unwrap();
    let mut path = vec![bend.parent().unwrap().to_path_buf()];
    path.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    let out = Command::new(bend).arg("run-c").arg(&file).env("PATH", std::env::join_paths(path).unwrap()).output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[test]
fn bedtime_windows_match_bend_spec() {
    let Some(b) = bend_or_skip() else { return };
    let src = include_str!("../../../bend/tests/window.bend").replace("return STEP", &format!("return {STEP}"));
    let out = run_bend(&b, &src, "window");
    let got = bend::parse_output(&out).unwrap_or_else(|| panic!("no result from bend:\n{out}"));
    let rows = (1440 / STEP) as usize;
    assert_eq!(got.len(), rows * 4, "bend returned {} numbers", got.len());
    for i in 0..rows {
        let want = rust_row(i as u16).map(|x| x as usize);
        assert_eq!(got[i * 4..i * 4 + 4], want, "start {} (row {i}): Bend spec and screen::Window disagree", window(i as u16, 0).start);
    }
}

/// Two different labels with the same 24-bit hash, so the audit's exact re-check has
/// something to throw out.
fn colliding_labels() -> (String, String) {
    let mut seen = HashMap::new();
    for n in 0u32.. {
        let l = format!("c{n}");
        if let Some(prev) = seen.insert(bend::label_hash(&l), l.clone()) {
            return (prev, l);
        }
    }
    unreachable!()
}

#[test]
fn audit_fuzz_bend_matches_exact_matcher() {
    if bend_or_skip().is_none() {
        return;
    }
    let mut rng = Rng(0x7a17_ea00_5eed);
    let (ca, cb) = colliding_labels();
    assert_ne!(ca, cb);
    assert_eq!(bend::label_hash(&ca), bend::label_hash(&cb));

    let words: Vec<String> = ["com", "net", "org", "uk", "co", "xxx", "app", "www", "cdn", "m", "api", "video", "chat", "img", "a-b", "x_y", "7"]
        .iter()
        .map(|s| s.to_string())
        .chain((0..40).map(|i| format!("w{i}")))
        .collect();
    let name = |rng: &mut Rng, n: usize| (0..n).map(|_| rng.pick(&words).clone()).collect::<Vec<_>>().join(".");

    let mut rules: Vec<String> = (0..250).map(|_| { let n = 2 + rng.below(2); name(&mut rng, n) }).collect();
    rules.push(format!("{ca}.com"));
    rules.push("*.Wild.Example.".into()); // normalized to wild.example
    rules.push("https://user@Rule.Example:8443/path".into()); // normalized to rule.example
    rules.push("not a host".into()); // dropped by both engines

    let mut hosts: Vec<String> = Vec::new();
    for _ in 0..6000 {
        let h = match rng.below(4) {
            // A rule with extra labels in front: should block.
            0 => {
                let n = rng.below(3);
                let r = rng.pick(&rules[..250]).clone();
                if n == 0 { r } else { format!("{}.{r}", name(&mut rng, n)) }
            }
            // Random names: some hit, most don't.
            1 | 2 => { let n = 1 + rng.below(5); name(&mut rng, n) }
            // URL and odd spellings of a rule.
            _ => {
                let r = rng.pick(&rules[..250]).clone();
                match rng.below(5) {
                    0 => format!("https://{}/x?y=1", r.to_uppercase()),
                    1 => format!("{r}:443"),
                    2 => format!("{r}."),
                    3 => format!("ftp://me@sub.{r}/"),
                    _ => format!("{r}.evil.test"), // rule is not a suffix: allowed
                }
            }
        };
        hosts.push(h);
    }
    // The collision: same hashes as a blocked rule, but a different name.
    hosts.push(format!("www.{cb}.com"));
    hosts.push(format!("{ca}.com"));
    hosts.push("deep.wild.example".into());
    hosts.push("rule.example".into());
    hosts.push("[::1]".into());
    hosts.push("".into());

    let a = bend::audit(&rules, &hosts, Engine::BendC);
    assert_eq!(a.engine, Engine::BendC, "Bend did not run (or failed and fell back to Rust)");
    let r = bend::audit(&rules, &hosts, Engine::Rust);

    let exact = DomainSet::new(&rules);
    let normalized: Vec<String> = hosts.iter().filter_map(|h| normalize_host(h)).collect();
    assert!(normalized.len() > bend::BATCH, "fuzz input should span several Bend batches");
    let want: Vec<(String, String)> = normalized
        .iter()
        .filter_map(|h| exact.matching_rule(h).map(|m| (h.clone(), m.to_string())))
        .collect();

    assert_eq!(a.checked, normalized.len());
    assert_eq!(a.blocked, want, "Bend audit disagrees with the exact matcher");
    assert_eq!(r.blocked, want, "Rust audit disagrees with the exact matcher");
    assert_eq!(a.false_hits, r.false_hits, "Bend reported different hash hits from the Rust model of the kernel");
    assert!(a.false_hits >= 1, "the planted hash collision should be caught by the exact re-check");
    assert!(want.iter().any(|(h, _)| h == "deep.wild.example"));
    assert!(want.iter().any(|(h, _)| h == "rule.example"));
    assert!(!want.iter().any(|(h, _)| h == &format!("www.{cb}.com")));
    eprintln!("audit fuzz: {} hosts, {} blocked, {} hash collisions re-checked away", a.checked, a.blocked.len(), a.false_hits);
}
