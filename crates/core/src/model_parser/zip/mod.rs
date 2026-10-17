use std::{fs::File, path::Path, sync::Arc};
mod gcode;
use anyhow::{Result, anyhow};
use tempfile::{TempDir, tempdir};
use tokio::fs;
use zip::ZipArchive;

use crate::{model_parser::zip::gcode::GCodeParser, types::model::Model};

/// Load zip model from file
pub async fn load_zip_model(zip_path: impl AsRef<std::path::Path>) -> Result<Arc<Model>> {
    let temp_dir = open_zip_temp(&zip_path)?;

    let gcode_file = temp_dir.path().join("run.gcode");
    let gcode = fs::read_to_string(gcode_file)
        .await
        .map_err(|e| anyhow!("Cannot open gcode file: {}", e))?;

    let mut gparser = GCodeParser::new();
    gparser.parse_gcode(gcode)?;

    let preview_path = temp_dir.path().join("preview.png");
    let p = Path::new(&preview_path);
    let preview = if p.exists() { Some(p) } else { None };

    let path: &Path = zip_path.as_ref();
    gparser.meta.file_name = path
        .file_name()
        .and_then(|n| Some(n.to_string_lossy().to_string()));

    Ok(Arc::new(Model::new(
        gparser.ir,
        gparser.meta,
        Arc::new(temp_dir),
        preview.map(|x| x.to_path_buf()),
    )))
}

/// Creates temp dir and extracts zip archive into it
fn open_zip_temp(zip_path: impl AsRef<std::path::Path>) -> Result<TempDir> {
    let temp_dir = tempdir().map_err(|e| anyhow!("Cannot create temp dir: {}", e))?;

    let file = File::open(zip_path).map_err(|e| anyhow!("Cannot open model .zip file: {}", e))?;
    let mut archive =
        ZipArchive::new(file).map_err(|e| anyhow!("Cannot get .zip model metadata: {}", e))?;

    archive
        .extract(temp_dir.path())
        .map_err(|e| anyhow!("Cannot unarchive .zip model: {}", e))?;

    Ok(temp_dir)
}
