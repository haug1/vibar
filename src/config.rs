use std::fs;

use serde::Deserialize;

use crate::modules::ModuleConfig;

#[derive(Debug, Deserialize, Clone, Default)]
pub(crate) struct Config {
    #[serde(default)]
    pub(crate) areas: Areas,
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

impl Default for Areas {
    fn default() -> Self {
        Self {
            left: default_left(),
            center: Vec::new(),
            right: default_right(),
        }
    }
}

fn default_left() -> Vec<ModuleConfig> {
    vec![crate::modules::sway::workspace::default_module_config()]
}

fn default_right() -> Vec<ModuleConfig> {
    vec![crate::modules::clock::default_module_config()]
}

pub(crate) fn load_config(path: &str) -> Config {
    match fs::read_to_string(path) {
        Ok(content) => match parse_config(&content) {
            Ok(cfg) => cfg,
            Err(err) => {
                eprintln!("Failed to parse {path}: {err}");
                Config::default()
            }
        },
        Err(_) => Config::default(),
    }
}

pub(crate) fn parse_config(content: &str) -> Result<Config, json5::Error> {
    json5::from_str::<Config>(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_config_missing_file_returns_defaults() {
        let cfg = load_config("./this-file-should-not-exist.jsonc");
        assert_eq!(cfg.areas.left.len(), 1);
        assert_eq!(cfg.areas.center.len(), 0);
        assert_eq!(cfg.areas.right.len(), 1);
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
}
