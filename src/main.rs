#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod border_service;
mod config;
mod diagnostics;
mod i18n;
mod own_window_frame;
mod rules;
mod single_instance;
mod startup;
mod ui_window_state;
mod watcher;
mod web_app;
mod window_manager;

fn main() -> anyhow::Result<()> {
    let arguments: Vec<_> = std::env::args().collect();
    if arguments
        .get(1)
        .is_some_and(|argument| argument == "--border-worker")
    {
        let address = arguments
            .get(2)
            .ok_or_else(|| anyhow::anyhow!("missing worker address"))?;
        let token = arguments
            .get(3)
            .ok_or_else(|| anyhow::anyhow!("missing worker token"))?;
        let ui_pid = arguments
            .get(4)
            .ok_or_else(|| anyhow::anyhow!("missing UI process"))?
            .parse()?;
        window_manager::set_ui_process_id(ui_pid);
        return border_service::run_worker(address, token);
    }
    let replace = arguments.iter().any(|argument| argument == "--replace");
    let elevated_task = arguments
        .iter()
        .any(|argument| argument == "--elevated-task");
    if !diagnostics::enabled() && !elevated_task && !replace && startup::launch_elevated_task()? {
        return Ok(());
    }
    let mut instance = single_instance::SingleInstance::acquire()?;
    if replace && !instance.is_primary() {
        // The old process restores its managed windows before exiting. A
        // replacement must not start a second watcher while that is happening.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !instance.is_primary() && std::time::Instant::now() < deadline {
            drop(instance);
            std::thread::sleep(std::time::Duration::from_millis(100));
            instance = single_instance::SingleInstance::acquire()?;
        }
    }
    if !instance.is_primary() {
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
