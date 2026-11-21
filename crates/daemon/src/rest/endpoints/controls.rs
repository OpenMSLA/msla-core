use actix_multipart::form::{MultipartForm, tempfile::TempFile};
use actix_web::{HttpResponse, Responder, post, web};
use msla_core::{
    model_parser::load_model_with_ext,
    types::{printer_manager::PrinterCommand, rest::RESTPrinterState},
};
use serde::Deserialize;
use serde_json::json;
use validator::Validate;

#[post("/abort")]
pub async fn abort_print(state: web::Data<RESTPrinterState>) -> impl Responder {
    match state.command_tx.send(PrinterCommand::Abort).await {
        Ok(_) => HttpResponse::Created().finish(),
        Err(_) => {
            HttpResponse::InternalServerError().json(json!({"message": "Cannot send abort signal"}))
        },
    }
}

#[derive(Debug, MultipartForm)]
struct UploadForm {
    #[multipart(limit = "100MB")]
    file: TempFile,
}

/// Starting a new print
#[post("/start")]
pub async fn start_print(
    state: web::Data<RESTPrinterState>,
    MultipartForm(form): MultipartForm<UploadForm>,
) -> impl Responder {
    if state.state.borrow().is_busy() {
        return HttpResponse::Conflict().json(
            json!({"message": "Printer is still printing. Abort print before starting new!"}),
        );
    }

    let file = form.file.file.path().to_path_buf();
    let Some(ext) = form
        .file
        .file_name
        .as_ref()
        .and_then(|f| f.split('.').next_back())
    else {
        return HttpResponse::BadRequest().json(json!({"message": "Cannot determine model type"}));
    };

    let model = match load_model_with_ext(file, ext).await {
        Ok(m) => m,
        Err(e) => {
            return HttpResponse::BadRequest().json(json!({"message": format!("{}", e)}));
        },
    };

    match state
        .command_tx
        .send(PrinterCommand::StartPrint(model))
        .await
    {
        Ok(()) => HttpResponse::Created().json(json!({"message": "Success"})),
        Err(e) => HttpResponse::InternalServerError()
            .json(json!({"message": format!("Cannot send command to daemon: {}", e)})),
    }
}

#[post("/home")]
pub async fn home(state: web::Data<RESTPrinterState>) -> impl Responder {
    if state.state.borrow().is_busy() {
        return HttpResponse::Conflict().json(json!({"message": "Printer is busy!"}));
    }

    match state.command_tx.send(PrinterCommand::Home).await {
        Ok(_) => HttpResponse::Created().json(json!({"message": "Home task signal sent"})),
        Err(_) => {
            HttpResponse::InternalServerError().json(json!({"message": "Cannot send home signal"}))
        },
    }
}

#[post("/disable-stepper")]
pub async fn dis_stepper(state: web::Data<RESTPrinterState>) -> impl Responder {
    if state.state.borrow().is_busy() {
        return HttpResponse::Conflict().json(json!({"message": "Printer is busy!"}));
    }

    match state.command_tx.send(PrinterCommand::DisableStepper).await {
        Ok(_) => {
            HttpResponse::Created().json(json!({"message": "Disable stepper task signal sent"}))
        },
        Err(_) => HttpResponse::InternalServerError()
            .json(json!({"message": "Cannot send disable stepper signal"})),
    }
}

#[derive(Deserialize, Validate)]
struct MoveTo {
    pub pos: f64,

    #[validate(range(min = 0.5, max = 300.0, message = "Invalid speed"))]
    pub speed: f64,
}

#[post("/move-to")]
pub async fn move_to(
    state: web::Data<RESTPrinterState>,
    item: web::Json<MoveTo>,
) -> impl Responder {
    if let Err(errors) = item.validate() {
        return HttpResponse::BadRequest().json(errors);
    }

    if state.state.borrow().is_busy() {
        return HttpResponse::Conflict().json(json!({"message": "Printer is busy!"}));
    }

    match state
        .command_tx
        .send(PrinterCommand::MoveTo {
            pos: item.pos,
            speed: item.speed,
        })
        .await
    {
        Ok(_) => HttpResponse::Created().json(json!({"message": "Moving command sent"})),
        Err(_) => HttpResponse::InternalServerError()
            .json(json!({"message": "Cannot send moving signal"})),
    }
}
