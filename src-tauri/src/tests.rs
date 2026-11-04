//! Contract tests for the shell's static wiring: the command list in
//! `build.rs`, `generate_handler!`, the per-window capabilities (SPEC §10
//! table) and the Tauri/bundle configuration.

use std::collections::BTreeSet;

use focus_core::events::{COMPANION_WINDOW, SETTINGS_WINDOW};
use serde_json::Value;

use super::COMMANDS;

const BUILD_RS: &str = include_str!("../build.rs");
const LIB_RS: &str = include_str!("lib.rs");
const COMPANION_CAP: &str = include_str!("../capabilities/companion.json");
const SETTINGS_CAP: &str = include_str!("../capabilities/settings.json");
const TAURI_CONF: &str = include_str!("../tauri.conf.json");
const TAURI_DEV_CONF: &str = include_str!("../tauri.dev.conf.json");
const INFO_PLIST: &str = include_str!("../Info.plist");

/// SPEC §10: commands each window may call.
const COMPANION_ALLOWED: [&str; 12] = [
    "get_snapshot",
    "start_focus",
    "start_break",
    "pause",
    "resume",
    "end_session",
    "dismiss",
    "get_active_pack",
    "replay_animation",
    "hide_companion",
    "open_settings",
    "get_app_info",
];
const SETTINGS_ALLOWED: [&str; 20] = [
    "get_snapshot",
    "start_focus",
    "start_break",
    "pause",
    "resume",
    "end_session",
    "dismiss",
    "update_settings",
    "set_hotkey",
    "set_hotkey_capture",
    "get_history",
    "list_packs",
    "get_active_pack",
    "set_active_pack",
    "replay_animation",
    "show_companion",
    "hide_companion",
    "reset_companion_position",
    "get_app_info",
    "quit_app",
];

fn quoted(s: &str) -> Vec<String> {
    s.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

fn set<'a>(it: impl IntoIterator<Item = &'a str>) -> BTreeSet<String> {
    it.into_iter().map(str::to_string).collect()
}

fn json(s: &str) -> Value {
    serde_json::from_str(s).expect("valid JSON")
}

#[test]
fn build_rs_lists_exactly_the_commands() {
    let start = BUILD_RS
        .find("let commands")
        .expect("command list in build.rs");
    let end = start + BUILD_RS[start..].find("];").expect("end of list");
    assert_eq!(quoted(&BUILD_RS[start..end]), COMMANDS.to_vec());
}

#[test]
fn generate_handler_registers_exactly_the_commands() {
    let start = LIB_RS
        .find("generate_handler![")
        .expect("generate_handler!");
    let end = start + LIB_RS[start..].find("])").expect("end of handler list");
    let names: Vec<String> = LIB_RS[start..end]
        .split("commands::")
        .skip(1)
        .map(|s| {
            s.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(names, COMMANDS.to_vec());
}

fn app_commands(cap: &Value) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut app = BTreeSet::new();
    let mut other = BTreeSet::new();
    for p in cap["permissions"].as_array().expect("permissions") {
        let p = p.as_str().expect("string permission");
        match p.strip_prefix("allow-") {
            Some(cmd) => app.insert(cmd.replace('-', "_")),
            None => other.insert(p.to_string()),
        };
    }
    (app, other)
}

#[test]
fn companion_capability_matches_the_spec_table() {
    let cap = json(COMPANION_CAP);
    assert_eq!(cap["windows"], serde_json::json!([COMPANION_WINDOW]));
    let (app, other) = app_commands(&cap);
    assert_eq!(app, set(COMPANION_ALLOWED));
    assert_eq!(
        other,
        set([
            "core:event:allow-listen",
            "core:event:allow-unlisten",
            "core:window:allow-start-dragging",
        ])
    );
}

#[test]
fn settings_capability_matches_the_spec_table() {
    let cap = json(SETTINGS_CAP);
    assert_eq!(cap["windows"], serde_json::json!([SETTINGS_WINDOW]));
    let (app, other) = app_commands(&cap);
    assert_eq!(app, set(SETTINGS_ALLOWED));
    assert_eq!(
        other,
        set(["core:event:allow-listen", "core:event:allow-unlisten"])
    );
}

#[test]
fn every_command_is_reachable_from_some_window() {
    let reachable: BTreeSet<String> = set(COMPANION_ALLOWED)
        .union(&set(SETTINGS_ALLOWED))
        .cloned()
        .collect();
    assert_eq!(reachable, set(COMMANDS));
}

#[test]
fn tauri_config_creates_windows_in_code_and_serves_packs() {
    let conf = json(TAURI_CONF);
    assert!(conf["productName"].as_str().is_some_and(|n| !n.is_empty()));
    assert_eq!(
        conf["app"]["windows"],
        serde_json::json!([]),
        "windows are built in code"
    );
    assert!(
        conf["app"].get("trayIcon").is_none(),
        "the tray is built in code, once"
    );
    assert_eq!(
        conf["app"]["macOSPrivateApi"], true,
        "matches the macos-private-api feature"
    );
    assert_eq!(conf["app"]["withGlobalTauri"], false);
    assert_eq!(conf["app"]["security"]["assetProtocol"]["enable"], false);
    for key in ["csp", "devCsp"] {
        let csp = conf["app"]["security"][key].as_str().expect(key);
        let img = csp
            .split(';')
            .find(|d| d.trim_start().starts_with("img-src"))
            .expect("img-src");
        assert!(
            img.contains("pack:") && img.contains("http://pack.localhost"),
            "{key}: {img}"
        );
        assert!(
            !csp.contains("http://*") && !csp.contains("https:"),
            "{key}: no remote origins"
        );
    }
}

#[test]
fn dev_config_never_touches_real_state() {
    let base = json(TAURI_CONF);
    let dev = json(TAURI_DEV_CONF);
    let id = base["identifier"].as_str().unwrap();
    assert_eq!(dev["identifier"].as_str().unwrap(), format!("{id}.dev"));
    let name = base["productName"].as_str().unwrap();
    assert_eq!(dev["productName"].as_str().unwrap(), format!("{name} Dev"));
}

#[test]
fn info_plist_hides_the_dock_icon() {
    let compact: String = INFO_PLIST.split_whitespace().collect();
    assert!(
        compact.contains("<key>LSUIElement</key><true/>"),
        "{INFO_PLIST}"
    );
}
