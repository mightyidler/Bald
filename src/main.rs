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
    let arguments: Vec<_> = std::env::args().collect();
    let elevated_task = arguments
        .iter()
        .any(|argument| argument == "--elevated-task");
    let replace = arguments.iter().any(|argument| argument == "--replace");
    if !elevated_task && startup::launch_elevated_task()? {
        return Ok(());
    }
    let instance = single_instance::SingleInstance::acquire()?;
    if !instance.is_primary() && !replace {
        let background = arguments
            .iter()
            .any(|argument| matches!(argument.as_str(), "--autostart" | "--background"));
        if !background {
            single_instance::show_existing();
        }
        return Ok(());
    }

    web_app::run(instance)
}
