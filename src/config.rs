use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::modules::ModuleConfig;

#[derive(Debug, Deserialize, Clone, Default)]
pub(crate) struct Config {
    #[serde(default)]
    pub(crate) areas: Areas,
    #[serde(default)]
    pub(crate) style: StyleConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct Areas {
    #[serde(default = "default_left")]
    pub(crate) left: Vec<ModuleConfig>,
    #[serde(default)]
    pub(crate) center: Vec<ModuleConfig>,
    #[serde(default = "default_right")]
    pub(crate) right: Vec<ModuleConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct StyleConfig {
    #[serde(
        rename = "load-default",
        alias = "load_default",
        default = "default_true"
    )]
    pub(crate) load_default: bool,
    #[serde(default, alias = "css-path", alias = "css_path")]
    pub(crate) path: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedConfig {
    pub(crate) config: Config,
    pub(crate) source_path: Option<PathBuf>,
}

impl Default for Areas {
    fn default() -> Self {
        Self {
            left: default_left(),
            center: Vec::new(),
            right: default_right(),
        }
    }
}

impl Default for StyleConfig {
    fn default() -> Self {
        Self {
            load_default: true,
            path: None,
        }
    }
}

fn default_left() -> Vec<ModuleConfig> {
    vec![crate::modules::sway::workspaces::default_module_config()]
}

fn default_right() -> Vec<ModuleConfig> {
    vec![crate::modules::clock::default_module_config()]
}

const PROJECT_CONFIG_PATH: &str = "./config.jsonc";

pub(crate) fn load_config() -> LoadedConfig {
    let candidate_paths = default_config_paths();
    load_config_from_paths(&candidate_paths)
}

fn default_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(path) = home_config_path() {
        paths.push(path);
    }

    paths.push(PathBuf::from(PROJECT_CONFIG_PATH));
    paths
}

fn home_config_path() -> Option<PathBuf> {
    if let Ok(xdg_config_home) = env::var("XDG_CONFIG_HOME") {
        return Some(
            PathBuf::from(xdg_config_home)
                .join("vibar")
                .join("config.jsonc"),
        );
    }

    env::var("HOME").ok().map(|home| {
        PathBuf::from(home)
            .join(".config")
            .join("vibar")
            .join("config.jsonc")
    })
}

fn load_config_from_paths(paths: &[PathBuf]) -> LoadedConfig {
    for path in paths {
        match fs::read_to_string(path) {
            Ok(content) => match parse_config(&content) {
                Ok(cfg) => {
                    return LoadedConfig {
                        config: cfg,
                        source_path: Some(path.clone()),
                    };
                }
                Err(err) => {
                    eprintln!("Failed to parse {}: {err}", path.display());
                }
            },
            Err(_) => continue,
        }
    }

    LoadedConfig {
        config: Config::default(),
        source_path: None,
    }
}

pub(crate) fn resolve_style_path(style_path: &str, config_source: Option<&Path>) -> PathBuf {
    if let Some(stripped) = style_path.strip_prefix("~/") {
        if let Ok(home) = env::var("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }

    let path = PathBuf::from(style_path);
    if path.is_absolute() {
        return path;
    }

    if let Some(source) = config_source {
        if let Some(parent) = source.parent() {
            return parent.join(path);
        }
    }

    path
}

fn default_true() -> bool {
    true
}

pub(crate) fn parse_config(content: &str) -> Result<Config, json5::Error> {
    json5::from_str::<Config>(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        env::temp_dir().join(format!("vibar-test-{name}-{nanos}.jsonc"))
    }

    #[test]
    fn load_config_missing_files_returns_defaults() {
        let cfg = load_config_from_paths(&[PathBuf::from("./this-file-should-not-exist.jsonc")]);
        assert_eq!(cfg.config.areas.left.len(), 1);
        assert_eq!(cfg.config.areas.center.len(), 0);
        assert_eq!(cfg.config.areas.right.len(), 1);
        assert!(cfg.source_path.is_none());
    }

    #[test]
    fn parse_config_applies_explicit_areas() {
        let cfg = parse_config(
            r#"{
                areas: {
                    left: [{ type: "clock" }],
                    center: [{ type: "clock" }],
                    right: [{ type: "clock" }]
                }
            }"#,
        )
        .expect("config should parse");

        assert_eq!(cfg.areas.left.len(), 1);
        assert_eq!(cfg.areas.center.len(), 1);
        assert_eq!(cfg.areas.right.len(), 1);
    }

    #[test]
    fn load_config_prefers_first_valid_path() {
        let home_cfg = test_path("home");
        let project_cfg = test_path("project");

        fs::write(
            &home_cfg,
            r#"{ areas: { left: [{ type: "exec", command: "echo home" }] } }"#,
        )
        .expect("home config should write");
        fs::write(
            &project_cfg,
            r#"{ areas: { left: [{ type: "exec", command: "echo project" }] } }"#,
        )
        .expect("project config should write");

        let loaded = load_config_from_paths(&[home_cfg.clone(), project_cfg.clone()]);

        assert_eq!(loaded.source_path.as_deref(), Some(home_cfg.as_path()));
        assert_eq!(loaded.config.areas.left[0].module_type, "exec");

        let _ = fs::remove_file(home_cfg);
        let _ = fs::remove_file(project_cfg);
    }

    #[test]
    fn load_config_falls_back_after_parse_error() {
        let home_cfg = test_path("home-invalid");
        let project_cfg = test_path("project-valid");

        fs::write(&home_cfg, "{ areas: ").expect("invalid home config should write");
        fs::write(
            &project_cfg,
            r#"{ areas: { right: [{ type: "clock", format: "%H:%M" }] } }"#,
        )
        .expect("project config should write");

        let loaded = load_config_from_paths(&[home_cfg.clone(), project_cfg.clone()]);

        assert_eq!(loaded.source_path.as_deref(), Some(project_cfg.as_path()));
        assert_eq!(loaded.config.areas.right[0].module_type, "clock");

        let _ = fs::remove_file(home_cfg);
        let _ = fs::remove_file(project_cfg);
    }

    #[test]
    fn resolve_style_path_expands_tilde() {
        let result = resolve_style_path("~/styles/vibar.css", None);
        assert!(result.is_absolute());
    }

    #[test]
    fn resolve_style_path_uses_config_parent_for_relative_paths() {
        let source = PathBuf::from("/tmp/vibar/config.jsonc");
        let result = resolve_style_path("style.local.css", Some(&source));
        assert_eq!(result, PathBuf::from("/tmp/vibar/style.local.css"));
    }
}
