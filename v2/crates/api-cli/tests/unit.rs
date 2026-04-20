use std::path::PathBuf;

use api_cli::{
    apply_install, apply_uninstall, describe_status, serialize_settings, InstallConfig,
    StatusReport,
};
use serde_json::{json, Value};

fn cfg(install_dir: &str) -> InstallConfig {
    InstallConfig {
        install_dir: PathBuf::from(install_dir),
        node_bin: PathBuf::from("/usr/bin/node"),
    }
}

// ─── install ─────────────────────────────────────────────────────────

#[test]
fn install_empty_settings_sets_all_three_keys() {
    let mut s = Value::Object(Default::default());
    apply_install(&mut s, &cfg("/opt/pm"));
    assert_eq!(
        s["extraKnownMarketplaces"]["project-man"]["source"]["path"],
        "/opt/pm"
    );
    assert_eq!(s["enabledPlugins"]["project-man@project-man"], true);
    assert_eq!(s["mcpServers"]["project-mem"]["type"], "stdio");
    assert_eq!(s["mcpServers"]["project-mem"]["command"], "/usr/bin/node");
    assert_eq!(
        s["mcpServers"]["project-mem"]["args"][0],
        "/opt/pm/plugin/mcp/server.cjs"
    );
}

#[test]
fn install_preserves_unrelated_user_keys() {
    let mut s = json!({
        "theme": "dark",
        "enabledPlugins": { "other@ns": true },
        "mcpServers": { "existing": { "type": "stdio", "command": "node" } }
    });
    apply_install(&mut s, &cfg("/opt/pm"));
    assert_eq!(s["theme"], "dark");
    assert_eq!(s["enabledPlugins"]["other@ns"], true);
    assert_eq!(s["enabledPlugins"]["project-man@project-man"], true);
    assert_eq!(s["mcpServers"]["existing"]["command"], "node");
    assert_eq!(s["mcpServers"]["project-mem"]["type"], "stdio");
}

#[test]
fn install_is_idempotent() {
    let mut s = Value::Object(Default::default());
    apply_install(&mut s, &cfg("/opt/pm"));
    let after_first = s.clone();
    apply_install(&mut s, &cfg("/opt/pm"));
    assert_eq!(after_first, s);
}

// ─── uninstall ───────────────────────────────────────────────────────

#[test]
fn uninstall_returns_false_if_nothing_to_remove() {
    let mut s = json!({ "theme": "dark" });
    assert!(!apply_uninstall(&mut s));
    assert_eq!(s, json!({ "theme": "dark" }));
}

#[test]
fn uninstall_drops_empty_top_keys() {
    let mut s = Value::Object(Default::default());
    apply_install(&mut s, &cfg("/opt/pm"));
    assert!(apply_uninstall(&mut s));
    assert!(s.get("extraKnownMarketplaces").is_none());
    assert!(s.get("enabledPlugins").is_none());
    assert!(s.get("mcpServers").is_none());
}

#[test]
fn uninstall_keeps_top_keys_with_other_entries() {
    let mut s = json!({
        "enabledPlugins": { "other@ns": true, "project-man@project-man": true },
        "mcpServers": { "existing": { "type": "stdio" }, "project-mem": { "type": "stdio" } }
    });
    assert!(apply_uninstall(&mut s));
    assert_eq!(s["enabledPlugins"]["other@ns"], true);
    assert!(s["enabledPlugins"].get("project-man@project-man").is_none());
    assert!(s["mcpServers"].get("project-mem").is_none());
    assert_eq!(s["mcpServers"]["existing"]["type"], "stdio");
}

#[test]
fn uninstall_roundtrip_with_install() {
    let mut s = json!({ "theme": "dark" });
    apply_install(&mut s, &cfg("/opt/pm"));
    assert!(apply_uninstall(&mut s));
    assert_eq!(s, json!({ "theme": "dark" }));
}

// ─── status ──────────────────────────────────────────────────────────

#[test]
fn status_installed() {
    let mut s = Value::Object(Default::default());
    apply_install(&mut s, &cfg("/opt/pm"));
    match describe_status(&s) {
        StatusReport::Installed { dir } => assert_eq!(dir, "/opt/pm"),
        other => panic!("expected Installed, got {other:?}"),
    }
}

#[test]
fn status_incomplete_missing_marketplace() {
    let s = json!({ "enabledPlugins": { "project-man@project-man": true } });
    assert_eq!(
        describe_status(&s),
        StatusReport::Incomplete {
            has_marketplace: false,
            has_plugin: true
        }
    );
}

#[test]
fn status_incomplete_missing_plugin() {
    let s = json!({
        "extraKnownMarketplaces": { "project-man": { "source": { "source": "directory", "path": "/x" } } }
    });
    assert_eq!(
        describe_status(&s),
        StatusReport::Incomplete {
            has_marketplace: true,
            has_plugin: false
        }
    );
}

#[test]
fn status_render_installed_matches_cjs_format() {
    let r = StatusReport::Installed {
        dir: "/opt/pm".to_string(),
    };
    let out = r.render(&PathBuf::from("/home/u/.claude/settings.json"));
    assert_eq!(
        out,
        "✓ project-man instalado\n  Diretório: /opt/pm\n  Settings:  /home/u/.claude/settings.json\n"
    );
}

#[test]
fn status_render_incomplete_lists_only_missing_items() {
    let r = StatusReport::Incomplete {
        has_marketplace: false,
        has_plugin: true,
    };
    let out = r.render(&PathBuf::from("/x"));
    assert_eq!(
        out,
        "✗ project-man não instalado (ou instalação incompleta)\n  → falta: extraKnownMarketplaces[\"project-man\"]\n  → rode: make install\n"
    );
}

// ─── serialize ───────────────────────────────────────────────────────

#[test]
fn serialize_uses_two_space_indent_and_trailing_newline() {
    let s = json!({ "a": 1, "b": [true, false] });
    let out = serialize_settings(&s);
    assert_eq!(
        out,
        "{\n  \"a\": 1,\n  \"b\": [\n    true,\n    false\n  ]\n}\n"
    );
}

#[test]
fn serialize_preserves_insertion_order() {
    let mut s = Value::Object(Default::default());
    apply_install(&mut s, &cfg("/opt/pm"));
    let out = serialize_settings(&s);
    // extraKnownMarketplaces inserted first, mcpServers last.
    let a = out.find("extraKnownMarketplaces").unwrap();
    let b = out.find("enabledPlugins").unwrap();
    let c = out.find("mcpServers").unwrap();
    assert!(a < b && b < c);
}
