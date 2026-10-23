use std::time::Duration;

use crate::status::format_duration;
use anyhow::Result;
use colored::Colorize;
use msla_core::{model_parser::load_model, types::cli::args::ModelInfoArgs};

/// Loads and prints model info
pub async fn print_model_info(model_info_args: &ModelInfoArgs) -> Result<()> {
    let model = load_model(model_info_args.path.clone()).await?;

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
