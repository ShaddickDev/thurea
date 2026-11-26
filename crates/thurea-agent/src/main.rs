//! `thurea` — family protection for Linux and Windows, in step with Thurea on Android.

mod accounts;
mod adopt;
mod browser;
mod config;
mod daemon;
mod control;
mod family;
mod platform;
mod prefs;
mod setup_phone;

use config::Config;
use thurea_core::bend::{self, Engine};
use thurea_core::{crypto, Policy, State};
use std::io::Read;

fn help() -> String {
    HELP.replace("{FAMILY}", family::HELP)
}

const HELP: &str = "\
Thurea — family protection for Linux, Windows and Android, working together.

Set up (as root / Administrator):
  thurea init [--name NAME]        Start a household on this computer; prints the pairing code
  thurea join CODE [--name NAME]   Join a household with its pairing code
  thurea install                   Run Thurea at startup (and start it now)
  thurea setup-phone [--name N] [--wifi SSID | --wifi-current] [--apk PATH] [--address IP]
                     [--save FILE.svg] [--tray]
                                   Set up a new or factory-reset phone with one QR scan: it
                                   installs Thurea from this computer, makes it the device
                                   owner and (as root, with a household) joins the household.
                                   Wi-Fi password: asked, or THUREA_WIFI_PASSWORD. --tray: the
                                   line format the tray apps and Omarchy plugin read
  thurea adopt-phone [--apk PATH] [--tray]
                                   Put Thurea on a phone that's in use, WITHOUT resetting it: pairs by
                                   QR (Wireless debugging) or USB, installs Thurea, signs the accounts
                                   out for a moment, makes Thurea the device owner, signs them back in.
                                   No admin needed; needs adb (android-tools)
  thurea pair-android [--name N] [--peers ADDR,...]
                                   Join the USB-connected Android phone that already has Thurea
  thurea add-peer ADDRESS          Also reach a device by address (e.g. a Tailscale IP)
  thurea set-relay HOST:PORT|off   Reach the household through its relay (before the family policy names one)
  thurea code                      Show the pairing code again

Every day:
  thurea status                    This computer's protection and every device in the household
  thurea watch                     Follow the household live (no admin needed; enforces nothing)
  thurea block SITE                Block a site on every device in the household
  thurea unblock SITE              Remove a site you added
  thurea check SITE                Is this site blocked?
  thurea audit FILE|- [--engine rust|bend|bend-gpu]
                                   Check a list of visited sites (one per line) against the
                                   blocklist (Rust by default; Bend runs it on HVM in parallel)

Family (a parent's controls; as root / Administrator):
{FAMILY}

Tray apps (no admin needed):
  thurea verse [--json]            Today's verse (World English Bible, public domain)
  thurea prefs [--json]            The tray apps' settings for this user
  thurea prefs set KEY on|off      verses (verse of the day), animation (logo on open)
  thurea desktop-status            This computer's protection, as JSON (for the tray apps)

Other:
  thurea apply                     Enforce the policy once and show the result
  thurea run                       The background service (started by `thurea install`)
  thurea uninstall                 Remove every rule Thurea set (recovery)
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let res = dispatch(&args);
    match res {
        Ok(out) => {
            if !out.is_empty() {
                println!("{out}");
            }
        }
        Err(e) => {
            eprintln!("thurea: {e}");
            std::process::exit(1);
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// Setting up or editing a config kept outside the system folders (THUREA_CONFIG_DIR,
/// e.g. an observer-only computer) needs no admin rights.
fn need_admin_for_config() -> Result<(), String> {
    if std::env::var_os("THUREA_CONFIG_DIR").is_some() { Ok(()) } else { need_admin() }
}

fn need_admin() -> Result<(), String> {
    if platform::is_admin() {
        Ok(())
    } else if cfg!(windows) {
        Err("run this from an Administrator terminal".into())
    } else {
        Err("run this as root (sudo thurea ...)".into())
    }
}

fn dispatch(args: &[String]) -> Result<String, String> {
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let arg = args.get(1).filter(|a| !a.starts_with("--")).cloned();
    match cmd {
        "init" | "join" => {
            need_admin_for_config()?;
            if config::config_path().exists() && !args.iter().any(|a| a == "--force") {
                return Err("this computer is already in a household (add --force to replace it)".into());
            }
            let key = if cmd == "join" {
                let code = arg.ok_or("usage: thurea join CODE")?;
                crypto::parse_key(&code).ok_or("that pairing code isn't valid (64 hex characters)")?
            } else {
                crypto::new_key().to_vec()
            };
            let cfg = Config::new(flag(args, "--name").unwrap_or_else(config::hostname), &key);
            cfg.save()?;
            if !config::policy_path().exists() {
                Policy::default().save(&config::policy_path()).map_err(|e| e.to_string())?;
            }
            Ok(format!(
                "{} is in the household as \"{}\".\n\nPairing code (keep it secret — it lets a device join):\n  {}\n\n{}",
                if cmd == "join" { "This computer" } else { "A new household was created. This computer" },
                cfg.name,
                cfg.pairing_code(),
                platform::start_mesh_service()
            ))
        }
        "code" => {
            need_admin_for_config()?;
            Ok(Config::load()?.pairing_code())
        }
        "set-relay" => {
            need_admin_for_config()?;
            let mut c = Config::load()?;
            let a = arg.ok_or("usage: thurea set-relay HOST:PORT|off")?;
            c.relay = if a == "off" { None } else { Some(a.clone()) };
            c.save()?;
            Ok(format!("Relay {}. Restart the service (or wait for it) to use it; a parent can also set it for every device with `thurea family relay`.", if a == "off" { "off".into() } else { a }))
        }
        "add-peer" => {
            need_admin_for_config()?;
            let a = arg.ok_or("usage: thurea add-peer ADDRESS")?;
            let mut cfg = Config::load()?;
            if !cfg.peers.contains(&a) {
                cfg.peers.push(a.clone());
            }
            cfg.save()?;
            Ok(format!("Added {a}. Restart Thurea to use it (sudo systemctl restart thurea / restart the computer)."))
        }
        "pair-android" => {
            need_admin()?;
            let cfg = Config::load()?;
            let name = flag(args, "--name").unwrap_or_else(|| "Phone".into());
            // adb joins its arguments into one shell line on the phone: quote each value.
            let q = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
            let mut cmd = format!("am start -n com.thurea.guard/.Remote --es mesh_key {} --es mesh_name {}", q(&cfg.key), q(&name));
            if let Some(p) = flag(args, "--peers") {
                cmd.push_str(&format!(" --es mesh_peers {}", q(&p)));
            }
            platform::run("adb", &["shell", &cmd])?;
            Ok(format!("Sent the household key to the phone over USB. It will appear as \"{name}\" in `thurea status` within a minute."))
        }
        "setup-phone" => {
            let tray = args.iter().any(|a| a == "--tray");
            let wifi = match flag(args, "--wifi") {
                None if args.iter().any(|a| a == "--wifi-current") => setup_phone::current_wifi(),
                None => None,
                // The tray apps pass the password in the environment, never on the command line.
                Some(ssid) if tray => Some((ssid, std::env::var("THUREA_WIFI_PASSWORD").unwrap_or_default())),
                Some(ssid) => Some((ssid.clone(), match std::env::var("THUREA_WIFI_PASSWORD") {
                    Ok(p) => p,
                    Err(_) => {
                        eprint!("Password for Wi-Fi \"{ssid}\" (it goes in the QR code only; empty if open): ");
                        let mut l = String::new();
                        std::io::stdin().read_line(&mut l).map_err(|e| e.to_string())?;
                        l.trim_end_matches(['\r', '\n']).to_string()
                    }
                })),
            };
            setup_phone::run(setup_phone::Options {
                apk: flag(args, "--apk").map(Into::into),
                address: flag(args, "--address").map(|a| a.parse().map_err(|_| format!("{a} isn't an IP address"))).transpose()?,
                port: flag(args, "--port").map(|p| p.parse().map_err(|_| "bad --port")).transpose()?.unwrap_or(setup_phone::HTTP_PORT),
                name: flag(args, "--name"),
                wifi,
                save: flag(args, "--save").map(Into::into),
                print_only: args.iter().any(|a| a == "--print-only"),
                tray,
            })
        }
        "adopt-phone" => adopt::run(flag(args, "--apk").map(Into::into), args.iter().any(|a| a == "--tray")),
        "install" => {
            need_admin()?;
            Config::load()?;
            // Windows: Thurea.ps1 -Install copies Thurea into place and applies everything itself.
            #[cfg(windows)]
            return platform::install().map(|m| format!("{m}\n\n{}", platform::check(&config::load_policy()).render()));
            #[cfg(not(windows))]
            {
                let r = platform::apply(&config::load_policy());
                Ok(format!("{}\n\n{}", platform::install()?, r.render()))
            }
        }
        "uninstall" => {
            need_admin()?;
            platform::uninstall()
        }
        "apply" => {
            need_admin()?;
            let r = platform::apply(&config::load_policy());
            Ok(r.render())
        }
        "run" => {
            need_admin_for_config()?;
            daemon::run(args.iter().any(|a| a == "--mesh-only")).map(|_| String::new())
        }
        "family" => family::run(args),
        // Chrome starts this as the browser extension's helper; nothing else may print.
        "family-host" => family::host(),
        "control-center" => control::run(args),
        "relay" => {
            let port: u16 = flag(args, "--port").map(|p| p.parse().map_err(|_| "port must be a number")).transpose()?.unwrap_or(thurea_core::relay::PORT);
            let stop = std::sync::atomic::AtomicBool::new(false);
            thurea_core::relay::serve(([0, 0, 0, 0], port).into(), &stop, &mut |l| eprintln!("{l}")).map_err(|e| e.to_string())?;
            Ok(String::new())
        }
        "status" => status(),
        "watch" => daemon::watch().map(|_| String::new()),
        "block" | "unblock" => {
            let site = arg.ok_or(format!("usage: thurea {cmd} SITE"))?;
            let block = cmd == "block";
            let current = family::current_policy();
            let mut p = current.clone();
            let changed = if block { p.block(&site)? } else { p.unblock(&site) };
            if !changed {
                return Ok(if block { format!("{site} is already blocked") } else { format!("{site} isn't on the list (only exact entries can be removed)") });
            }
            let done = if block { "Blocked" } else { "Unblocked" };
            if current.family.parents.is_empty() {
                need_admin_for_config()?;
                p.save(&config::policy_path()).map_err(|e| e.to_string())?;
                return Ok(format!("{done} {site}. Policy is now version {}; every device in the household picks it up within a minute.", p.version));
            }
            // A family: only a parent can, and signs it.
            family::change(|p| {
                if block {
                    let _ = p.block(&site);
                } else {
                    p.unblock(&site);
                }
            })
            .map(|m| format!("{done} {site}. {m}"))
        }
        "check" => {
            let site = arg.ok_or("usage: thurea check SITE")?;
            let p = config::load_policy();
            Ok(match p.domains().matching_rule(&site) {
                Some(rule) => format!("blocked (rule: {rule})"),
                None => "not on the blocklist (family DNS may still filter it)".into(),
            })
        }
        "audit" => {
            let src = arg.ok_or("usage: thurea audit FILE|-")?;
            let mut text = String::new();
            if src == "-" {
                std::io::stdin().read_to_string(&mut text).map_err(|e| e.to_string())?;
            } else {
                text = std::fs::read_to_string(&src).map_err(|e| format!("{src}: {e}"))?;
            }
            let engine = match flag(args, "--engine").as_deref() {
                None | Some("rust") => Engine::Rust,
                Some("bend") => Engine::BendC,
                Some("bend-gpu") => Engine::BendCuda,
                Some(o) => return Err(format!("unknown engine {o}")),
            };
            let inputs: Vec<String> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(String::from).collect();
            let p = config::load_policy();
            let t = std::time::Instant::now();
            let a = bend::audit(&p.blocklist, &inputs, engine);
            let mut out = format!("Checked {} sites with {} in {:.2?}: {} blocked", a.checked, a.engine.name(), t.elapsed(), a.blocked.len());
            if a.false_hits > 0 {
                out.push_str(&format!(" ({} hash collisions discarded by the exact check)", a.false_hits));
            }
            if engine != Engine::Rust && a.engine == Engine::Rust {
                out.push_str("\n(Bend isn't installed or failed — used the Rust engine. Install: cargo install hvm bend-lang)");
            }
            let mut by_rule = std::collections::BTreeMap::<String, usize>::new();
            for (_, rule) in &a.blocked {
                *by_rule.entry(rule.clone()).or_default() += 1;
            }
            let mut top: Vec<_> = by_rule.into_iter().collect();
            top.sort_by(|a, b| b.1.cmp(&a.1));
            for (rule, n) in top.iter().take(20) {
                out.push_str(&format!("\n  {n:>6}  {rule}"));
            }
            Ok(out)
        }
        "verse" => {
            let v = thurea_core::verses::today(prefs::utc_offset());
            let (abbr, notice) = thurea_core::verses::translation();
            Ok(if args.iter().any(|a| a == "--json") {
                serde_json::json!({ "ref": v.reference, "text": v.text, "translation": abbr, "notice": notice, "show": prefs::get("verses") }).to_string()
            } else {
                format!("\u{201c}{}\u{201d}\n  {} ({abbr})", v.text, v.reference)
            })
        }
        "prefs" => match (args.get(1).map(String::as_str), args.get(2), args.get(3).map(String::as_str)) {
            (Some("set"), Some(k), Some(v @ ("on" | "off" | "true" | "false"))) => {
                prefs::set(k, v == "on" || v == "true")?;
                Ok(String::new())
            }
            (Some("set"), _, _) => Err("usage: thurea prefs set KEY on|off".into()),
            _ if args.iter().any(|a| a == "--json") => Ok(serde_json::Value::Object(prefs::load()).to_string()),
            _ => Ok(prefs::load().iter().map(|(k, v)| format!("{k:<10} {}", if v.as_bool() == Some(true) { "on" } else { "off" })).collect::<Vec<_>>().join("\n")),
        },
        "desktop-status" => Ok(platform::desktop_status().to_string()),
        "help" | "--help" | "-h" => Ok(help()),
        "version" | "--version" => Ok(format!("thurea {}", env!("CARGO_PKG_VERSION"))),
        other => Err(format!("unknown command {other:?}\n\n{}", help())),
    }
}

fn ago(t: u64) -> String {
    let s = thurea_core::now().saturating_sub(t);
    match s {
        0..=59 => format!("{s}s ago"),
        60..=3599 => format!("{}m ago", s / 60),
        _ => format!("{}h ago", s / 3600),
    }
}

fn status() -> Result<String, String> {
    let raw = std::fs::read_to_string(config::state_path()).map_err(|_| {
        "Thurea isn't running on this computer (no status yet). Set it up with `thurea init` or `thurea join`, then `thurea install`.".to_string()
    })?;
    let v: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let mut out = String::new();
    let updated = v["updated"].as_u64().unwrap_or(0);
    let stale = thurea_core::now().saturating_sub(updated) > 180;
    out.push_str(&format!("{} — policy v{} — checked {}\n", v["name"].as_str().unwrap_or("?"), v["policy_version"], ago(updated)));
    if stale {
        out.push_str("! Thurea's service hasn't reported for over 3 minutes — is it running?\n");
    }
    if let Some(checks) = v["report"]["checks"].as_array() {
        for c in checks {
            let st: State = serde_json::from_value(c["state"].clone()).unwrap_or(State::Unknown);
            out.push_str(&format!("{} {}\n", st.mark(), c["detail"].as_str().unwrap_or("")));
        }
    }
    out.push_str(&format!("\nHousehold: {}\n", v["household"].as_str().unwrap_or("")));
    if let Some(peers) = v["peers"].as_array() {
        for p in peers {
            let st: State = serde_json::from_value(p["state"].clone()).unwrap_or(State::Unknown);
            out.push_str(&format!(
                "  {} {} ({}, policy v{}) — seen {}\n",
                st.mark(),
                p["name"].as_str().unwrap_or("?"),
                p["platform"].as_str().unwrap_or("?"),
                p["policy_version"],
                ago(p["last_seen"].as_u64().unwrap_or(0))
            ));
            for prob in p["problems"].as_array().into_iter().flatten() {
                out.push_str(&format!("      {}\n", prob.as_str().unwrap_or("")));
            }
        }
    }
    if let Some(recent) = v["recent"].as_array().filter(|r| !r.is_empty()) {
        out.push_str("\nRecent:\n");
        for e in recent.iter().rev().take(8) {
            out.push_str(&format!("  {} — {}\n", ago(e[0].as_u64().unwrap_or(0)), e[1].as_str().unwrap_or("")));
        }
    }
    Ok(out.trim_end().to_string())
}
