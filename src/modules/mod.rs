pub(crate) mod clock;
pub(crate) mod exec;
pub(crate) mod sway;

use gtk::Widget;

use crate::config::ModuleConfig;

pub(crate) trait ModuleFactory {
    fn init(&self, config: &ModuleConfig) -> Option<Widget>;
}

const FACTORIES: &[&dyn ModuleFactory] =
    &[&exec::FACTORY, &sway::workspace::FACTORY, &clock::FACTORY];

pub(crate) fn build_module(config: &ModuleConfig) -> Option<Widget> {
    FACTORIES.iter().find_map(|factory| factory.init(config))
}
