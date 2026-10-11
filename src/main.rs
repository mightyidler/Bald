#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod border_service;
mod config;
mod i18n;
mod icon_cache;
mod own_window_frame;
mod rules;
mod single_instance;
mod startup;
mod ui_activation;
mod ui_window_state;
mod watcher;
mod web_app;
mod window_manager;

fn main() -> anyhow::Result<()> {
    let arguments: Vec<_> = std::env::args().collect();
    if arguments
        .iter()
        .any(|argument| argument == "--remove-elevated-task")
    {
        return startup::remove_elevated_task();
    }
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
    let mut instance = single_instance::SingleInstance::acquire()?;
    if replace && !instance.is_primary() {
        // The old process restores its managed windows before exiting. A
        // replacement must not start a second watcher while that is happening.
        instance.wait_for_primary()?;
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

    if startup::should_launch_elevated_task(&arguments, elevated_task, replace) {
        // Release before launching the task, so its process can become primary.
        drop(instance);
        if startup::launch_elevated_task()? {
            return Ok(());
        }
        instance = single_instance::SingleInstance::acquire()?;
        if !instance.is_primary() {
            return Ok(());
        }
    }

    web_app::run(instance)
}
