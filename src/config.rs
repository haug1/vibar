use std::fs;

use serde::Deserialize;

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
    vec![ModuleConfig::Workspaces]
}

fn default_right() -> Vec<ModuleConfig> {
    vec![ModuleConfig::Clock { format: None }]
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(crate) enum ModuleConfig {
    Exec {
        command: String,
        #[serde(default = "default_exec_interval")]
        interval_secs: u32,
        #[serde(default)]
        class: Option<String>,
    },
    Workspaces,
    Clock {
        #[serde(default)]
        format: Option<String>,
    },
}

fn default_exec_interval() -> u32 {
    5
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
