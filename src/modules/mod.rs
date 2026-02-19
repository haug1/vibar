pub(crate) mod clock;
pub(crate) mod exec;
pub(crate) mod sway;

use gtk::Widget;
use serde::Deserialize;

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

pub(crate) trait ModuleFactory {
    fn init(&self, config: &ModuleConfig) -> Option<Widget>;
}

const FACTORIES: &[&dyn ModuleFactory] =
    &[&exec::FACTORY, &sway::workspace::FACTORY, &clock::FACTORY];

pub(crate) fn build_module(config: &ModuleConfig) -> Option<Widget> {
    FACTORIES.iter().find_map(|factory| factory.init(config))
}
