use crate::rest::endpoints::{
    controls::{abort_print, dis_stepper, home, move_to, pause, resume, start_print},
    status::{get_preview, get_status},
};
use actix_web::{App, HttpServer, dev::Server, web};
use anyhow::Result;
use msla_core::types::{
    printer_manager::{PrinterCommand, PrinterState},
    rest::RESTPrinterState,
};
use std::net::SocketAddrV4;
use tokio::sync::{mpsc::Sender, watch::Receiver};

pub mod endpoints;

/// Build HTTP server
pub fn build_rest_api(
    addr: SocketAddrV4,
    state_rx: Receiver<PrinterState>,
    command_tx: Sender<PrinterCommand>,
) -> Result<Server> {
    let app_state = web::Data::new(RESTPrinterState {
        state: state_rx,
        command_tx,
    });
    let app = move || {
        App::new()
            .app_data(app_state.clone())
            .service(get_status)
            .service(abort_print)
            .service(start_print)
            .service(get_preview)
            .service(home)
            .service(dis_stepper)
            .service(move_to)
            .service(pause)
            .service(resume)
    };

    Ok(HttpServer::new(app).bind(addr)?.run())
}
