pub(crate) mod clock;
pub(crate) mod exec;
pub(crate) mod sway;
pub(crate) mod tray;

use gtk::Widget;
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Debug, Clone, Default)]
pub(crate) struct ModuleBuildContext {
    pub(crate) monitor_connector: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct ModuleConfig {
    #[serde(rename = "type")]
    pub(crate) module_type: String,
    #[serde(flatten, default)]
    pub(crate) config: Map<String, Value>,
}

impl ModuleConfig {
    pub(crate) fn new(module_type: impl Into<String>, config: Map<String, Value>) -> Self {
        Self {
            module_type: module_type.into(),
            config,
        }
    }
}

pub(crate) trait ModuleFactory {
    fn module_type(&self) -> &'static str;
    fn init(&self, config: &ModuleConfig, context: &ModuleBuildContext) -> Result<Widget, String>;
}

const FACTORIES: &[&dyn ModuleFactory] = &[
    &exec::FACTORY,
    &sway::workspace::FACTORY,
    &clock::FACTORY,
    &tray::FACTORY,
];

pub(crate) fn build_module(
    config: &ModuleConfig,
    context: &ModuleBuildContext,
) -> Result<Widget, String> {
    let factory = FACTORIES
        .iter()
        .find(|factory| factory.module_type() == config.module_type)
        .ok_or_else(|| format!("unknown module type '{}'", config.module_type))?;

    factory.init(config, context)
}

#[cfg(test)]
mod tests {
    use serde_json::Map;

    use super::*;

    #[test]
    fn build_module_rejects_unknown_module_type() {
        let module = ModuleConfig::new("does-not-exist", Map::new());
        let err = build_module(&module, &ModuleBuildContext::default())
            .expect_err("unknown module should fail");
        assert!(err.contains("unknown module type 'does-not-exist'"));
    }
}
