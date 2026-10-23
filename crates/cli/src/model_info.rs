use std::time::Duration;

use crate::status::format_duration;
use anyhow::Result;
use colored::Colorize;
use msla_core::{
    model_parser::{photon::load_photon_model, zip::load_zip_model},
    types::cli::args::ModelInfoArgs,
};
use tracing::error;

/// Loads and prints model info
pub async fn print_model_info(model_info_args: &ModelInfoArgs) -> Result<()> {
    let model = if model_info_args.zip {
        load_zip_model(model_info_args.path.clone()).await?
    } else if model_info_args.photon {
        load_photon_model(model_info_args.path.clone()).await?
    } else {
        error!("Please, provide file extension");
        return Ok(());
    };

    let lines = [
        format!(
            "{}: {}",
            "Model name".bold(),
            model
                .model_meta
                .file_name
                .clone()
                .unwrap_or("unknown".to_string())
        ),
        format!(
            "{}: {}",
            "Layer count".bold(),
            model.model_meta.total_layer_count.clone()
        ),
        format!("{}: {:.3}", "IR len".bold(), model.ir.len()),
        format!(
            "{}: {}",
            "Estimated print time".bold(),
            format_duration(Duration::from_secs(
                model.model_meta.estimated_printing_time as u64
            ))
        ),
        format!(
            "{}: {:.3}",
            "Volume".bold(),
            model.model_meta.volume.unwrap_or(0.0)
        ),
    ];

    println!("{}", lines.join("\n"));
    Ok(())
}
