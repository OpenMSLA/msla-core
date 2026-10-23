use crate::{
    model_parser::{photon::load_photon_model, zip::load_zip_model},
    types::model::Model,
};
use anyhow::{Result, anyhow};
use std::sync::Arc;

pub mod photon;
pub mod zip;

/// Determine type and load model file
pub async fn load_model(path: impl AsRef<std::path::Path>) -> Result<Arc<Model>> {
    let p = path.as_ref();
    let ext = p
        .extension()
        .and_then(|ext| ext.to_str())
        .ok_or(anyhow!("Cannot determine model extension"))?;

    load_model_with_ext(p, ext).await
}

/// Load model file with provided ext
pub async fn load_model_with_ext(
    path: impl AsRef<std::path::Path>,
    ext: &str,
) -> Result<Arc<Model>> {
    let p = path.as_ref();

    Ok(match ext {
        "zip" => load_zip_model(p).await?,
        "photon" => load_photon_model(p).await?,
        _ => anyhow::bail!("Invalid file ext"),
    })
}
