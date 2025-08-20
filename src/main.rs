use crate::{komo::listen_for_workspaces, window::Window};

mod consts;
mod komo;
mod window;
mod workspaces;
mod msgs;

fn begin_execution() -> anyhow::Result<()> {
    log::info!("Starting execution...");
    // Here you can add any initialization code needed before the main loop starts.
    let mut window = Window::new()?;

    window.prepare()?;

    let hwnd = unsafe { window.hwnd.raw_copy() };
    std::thread::spawn(move || listen_for_workspaces(hwnd));

    window.run_loop()
}

fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .format_timestamp(None)
        .format_file(true)
        .format_line_number(true)
        .init();

    begin_execution().unwrap_or_else(|err| {
        println!("{:?}", err.backtrace());
        log::error!("Application error: {}", err);
    });

    Ok(())
}
