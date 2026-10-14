mod broadcast;
mod control_display;
mod lcd;
mod peripheral;
mod printer_manager;
mod rest;
mod sys_fan;
mod uart;
mod usb_service;

use std::{
    net::{Ipv4Addr, SocketAddrV4},
    str::FromStr,
    sync::Arc,
};

use anyhow::Result;
use msla_core::{
    config,
    logging,
    shutdown::shutdown_signal,
    types::printer_manager::{PrinterCommand, PrinterState},
};
use tokio::sync::{Notify, mpsc, watch};
use tracing::{Level, error, info};

use crate::{
    broadcast::start_broadcast,
    control_display::spawn_control_display_thread,
    lcd::LCDController,
    peripheral::PeripheralController,
    printer_manager::PrinterManager,
    rest::build_rest_api,
    sys_fan::start_fan,
    uart::Uart,
    usb_service::start_usb_service,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Set up logger
    let reload_handle = logging::init_logger(tracing::Level::DEBUG, true);

    // Load config from file
    config::load("./config.toml").await.map_err(|e| {
        error!("{}", e);
        std::process::exit(-1);
    });

    let config = config::get_config().await;

    logging::set_log_level(
        reload_handle,
        logging::str_to_log_level(&config.global.logging_level).unwrap_or(Level::INFO),
    );

    // Create printer command channel - main communication tunnel
    // between parts of code
    let (command_tx, command_rx) = mpsc::channel::<PrinterCommand>(128);

    // Global printer state
    let (state_tx, state_rx) = watch::channel(PrinterState::default());

    // Create peripheral controller - a bridge between high-level code
    // and middle- and low-level protocol
    let peripheral_controller = PeripheralController::new().await?;

    // Create lcd controller - wrapper around the linux framebuffer
    let lcd_controller = LCDController::new().await?;

    // Start fan controlling cycle
    let fan_controller = peripheral_controller.clone();
    tokio::spawn(async { start_fan(fan_controller).await });

    // Start usb mounter service task
    tokio::spawn(async { start_usb_service().await });

    // Build and run REST API
    let rest = build_rest_api(
        SocketAddrV4::new(
            Ipv4Addr::from_str(&config.rest_api.addr)?,
            config.rest_api.port,
        ),
        state_rx.clone(),
        command_tx.clone(),
    )?;

    tokio::spawn(rest);

    // Spawn thread for UI display
    let display_uart = Uart::open(
        config.control_display.uart.clone(),
        config.control_display.baud_rate,
    );

    match display_uart {
        Ok(du) => {
            spawn_control_display_thread(du, command_tx, state_rx, peripheral_controller.clone())?
        },
        Err(e) => error!("Display connection failed: {e}, starting without it..."),
    }

    // Start broadcast server
    tokio::spawn(async { start_broadcast().await });

    // Some graceful shutdown things
    let shutdown_notify = Arc::new(Notify::new());

    // Graceful shutdown notifier task
    tokio::spawn({
        let shutdown_notify = shutdown_notify.clone();

        async move {
            shutdown_signal().await;
            shutdown_notify.notify_one();
        }
    });

    // Create printer manager instance and run it
    let mut printer =
        PrinterManager::new(command_rx, state_tx, peripheral_controller, lcd_controller);

    tokio::select! {
        _ = printer.run() => {},
        _ = shutdown_notify.notified() => {
            info!("Shutting down the server...");
        }
    }
    Ok(())
}
