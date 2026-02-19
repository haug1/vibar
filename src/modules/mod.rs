pub(crate) mod clock;
pub(crate) mod exec;
pub(crate) mod sway;

use gtk::prelude::*;
use gtk::Widget;

use crate::config::ModuleConfig;

trait BarModule {
    fn init(&self) -> Widget;
}

struct ExecModule {
    command: String,
    interval_secs: u32,
    class: Option<String>,
}

impl BarModule for ExecModule {
    fn init(&self) -> Widget {
        exec::build_exec_module(self.command.clone(), self.interval_secs, self.class.clone())
            .upcast()
    }
}

struct ClockModule {
    format: Option<String>,
}

impl BarModule for ClockModule {
    fn init(&self) -> Widget {
        clock::build_clock_module(self.format.clone()).upcast()
    }
}

struct SwayWorkspaceModule;

impl BarModule for SwayWorkspaceModule {
    fn init(&self) -> Widget {
        sway::workspace::build_workspaces_module().upcast()
    }
}

fn module_from_config(config: &ModuleConfig) -> Box<dyn BarModule> {
    match config {
        ModuleConfig::Exec {
            command,
            interval_secs,
            class,
        } => Box::new(ExecModule {
            command: command.clone(),
            interval_secs: *interval_secs,
            class: class.clone(),
        }),
        ModuleConfig::Workspaces => Box::new(SwayWorkspaceModule),
        ModuleConfig::Clock { format } => Box::new(ClockModule {
            format: format.clone(),
        }),
    }
}

pub(crate) fn build_module(config: &ModuleConfig) -> Widget {
    module_from_config(config).init()
}
