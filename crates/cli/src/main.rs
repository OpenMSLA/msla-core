use crate::{
    api::{ApiService, PlacingType},
    context::{add_to_context, remove_from_context},
    model_info::print_model_info,
    printer_selector::run_selector,
    status::show_status,
};
use anyhow::Result;
use clap::Parser;
use msla_core::{
    dyn_config::DynConfig,
    logging,
    modify_config,
    types::cli::{
        args::{Args, Command},
        dyn_config::CliDynConfigType,
    },
};
use notify_rust::{Notification, Urgency};
use std::{path::PathBuf, process};
use tracing::{error, info};

mod api;
mod context;
mod model_info;
mod printer_selector;
mod search;
mod status;

#[cfg(windows)]
fn set_console_visible(visible: bool) {
    use windows::Win32::{
        System::Console::GetConsoleWindow,
        UI::WindowsAndMessaging::{SW_HIDE, SW_SHOW, ShowWindow},
    };

    unsafe {
        let hwnd = GetConsoleWindow();

        if !hwnd.is_invalid() {
            let _ = ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE });
        }
    }
}

#[cfg(not(windows))]
fn set_console_visible(visible: bool) {}

/// --------------------- DYN CONFIG THINGS --------------------

static CONFIG: DynConfig<CliDynConfigType> = DynConfig::new();

#[tokio::main]
async fn main() -> Result<()> {
    logging::init_logger(tracing::Level::INFO, false);
    CONFIG.init_config().await?;
    let args = Args::parse();

    if args.from_context_menu {
        set_console_visible(false);
    }

    match execute(&args).await {
        Ok(()) if args.from_context_menu => {
            Notification::new()
                .summary("Open MSLA")
                .body("Command successfully sent!")
                .appname("msla-cli")
                .show()?;
            return Ok(());
        },

        Err(e) if args.from_context_menu => {
            Notification::new()
                .summary("Open MSLA")
                .body(&e.to_string())
                .appname("msla-cli")
                .urgency(Urgency::Critical)
                .show()?;
            return Err(e);
        },

        r => return r,
    }
}

async fn execute(args: &Args) -> Result<()> {
    match &args.command {
        Command::ContextRegister => {
            add_to_context("Send to printer").await?;
            return Ok(());
        },
        Command::ContextUnregister => {
            remove_from_context().await?;
            return Ok(());
        },
        Command::ModelInfo(model_info_args) => print_model_info(model_info_args).await?,

        Command::ChangeConfig(config) => {
            match config.always_alt_scan.as_deref() {
                Some("true") | Some("t") => modify_config!(CONFIG, connectivity.alt_default = true),
                Some("false") | Some("f") => {
                    modify_config!(CONFIG, connectivity.alt_default = false)
                },
                Some(_) => {
                    error!("Cannot parse `always_alt_scan`. Use `true` | `false`");
                    process::exit(-1);
                },
                None => {},
            }

            if let Some(port) = config.default_port {
                modify_config!(CONFIG, connectivity.default_port = port);
            }

            if let Some(port) = config.default_scan_port {
                modify_config!(CONFIG, connectivity.default_scan_port = port);
            }

            CONFIG.store_config().await?;
            info!("Config updated!");
        },

        command => {
            let config = CONFIG.config()?.read().await;
            let host = match args.host.clone() {
                Some(host) => host,
                None => {
                    if args.from_context_menu {
                        set_console_visible(true);
                    }

                    run_selector(
                        config.connectivity.alt_default || args.alt_scan,
                        args.scan_port
                            .unwrap_or(config.connectivity.default_scan_port),
                    )
                    .await?
                    .ip
                    .to_string()
                },
            };

            if args.from_context_menu {
                set_console_visible(false);
            }

            let api_client =
                ApiService::new(host, args.port.unwrap_or(config.connectivity.default_port));

            match command {
                Command::Start(start_args) => {
                    if let Some(local) = &start_args.local {
                        api_client
                            .start_print(PlacingType::Local, PathBuf::from(local))
                            .await?;
                    } else if let Some(remote) = &start_args.remote {
                        api_client
                            .start_print(PlacingType::Remote, PathBuf::from(remote))
                            .await?;
                    } else {
                        error!("Please, specify the file path: --local <path> or --remote <path>")
                    }

                    println!("Print started! Enjoy the spectacle of printing :)");
                    if start_args.watch {
                        show_status(&api_client, true, 10).await?;
                    }
                },
                Command::Abort => {
                    api_client.abort().await?;
                    println!("Print aborted");
                },
                Command::Pause => todo!(),
                Command::Resume => todo!(),
                Command::Status(status_args) => {
                    show_status(
                        &api_client,
                        status_args.watch,
                        status_args.period.unwrap_or(10),
                    )
                    .await?;
                },
                Command::Home => {
                    api_client.home().await?;
                    println!("Printer homing...");
                },
                Command::DisableStepper => {
                    api_client.disable_stepper().await?;
                    println!("Steppers disabled...");
                },
                Command::ShowPreview => {
                    info!("Open url: {}/preview", api_client.url)
                },

                Command::MoveTo(move_to) => {
                    api_client
                        .move_to(move_to.pos, move_to.speed.unwrap_or(50.0))
                        .await?;
                    println!("Printer start moving...");
                },

                Command::ContextRegister
                | Command::ContextUnregister
                | Command::ModelInfo(_)
                | Command::ChangeConfig(_) => {
                    unreachable!()
                },
            }
        },
    }

    Ok(())
}
