#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod i18n;
mod rules;
mod single_instance;
mod startup;
mod watcher;
mod web_app;
mod window_manager;

fn main() -> anyhow::Result<()> {
    let instance = single_instance::SingleInstance::acquire()?;
    if !instance.is_primary() {
        single_instance::show_existing();
        return Ok(());
    }

    web_app::run(instance)
}
