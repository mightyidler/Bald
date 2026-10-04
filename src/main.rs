#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod config;
mod i18n;
mod rules;
mod single_instance;
mod startup;
mod watcher;
mod web_app;
mod window_manager;

fn main() -> anyhow::Result<()> {
    let elevated_task = std::env::args().any(|argument| argument == "--elevated-task");
    if !elevated_task && startup::launch_elevated_task()? {
        return Ok(());
    }
    let instance = single_instance::SingleInstance::acquire()?;
    if !instance.is_primary() {
        single_instance::show_existing();
        return Ok(());
    }

    web_app::run(instance)
}
