pub(crate) mod clock;
pub(crate) mod exec;
pub(crate) mod sway;

use gtk::Widget;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(crate) enum ModuleConfig {
    Exec {
        #[serde(flatten)]
        config: exec::ExecConfig,
    },
    Workspaces {
        #[serde(flatten)]
        config: sway::workspace::WorkspaceConfig,
    },
    Clock {
        #[serde(flatten)]
        config: clock::ClockConfig,
    },
}

pub(crate) trait ModuleFactory {
    fn init(&self, config: &ModuleConfig) -> Option<Widget>;
}

const FACTORIES: &[&dyn ModuleFactory] =
    &[&exec::FACTORY, &sway::workspace::FACTORY, &clock::FACTORY];

pub(crate) fn build_module(config: &ModuleConfig) -> Option<Widget> {
    FACTORIES.iter().find_map(|factory| factory.init(config))
}
