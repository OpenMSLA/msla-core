use anyhow::{Result, anyhow};
use image::{DynamicImage, GenericImageView, Rgb, RgbImage, imageops::FilterType};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::PathBuf,
    sync::{Arc, LazyLock, Mutex},
};
use tracing::error;

#[derive(Default)]
pub struct LoadedImage {
    pub data: Vec<u8>,
    pub hash: u64,
}

static LOADED_PREVIEW: LazyLock<Mutex<Arc<LoadedImage>>> =
    LazyLock::new(|| Mutex::new(Arc::new(LoadedImage::default())));

/// Calcs path hash
fn calc_path_hash(pb: &PathBuf) -> u64 {
    let mut s = DefaultHasher::new();
    pb.hash(&mut s);
    s.finish()
}

pub fn get_preview_bytes(
    path: Option<PathBuf>,
    side: u16,
    count: u16,
    offset: u16,
) -> Result<Vec<u8>> {
    let path = path.ok_or(anyhow!("Preview not available"))?;
    let mut li = {
        match LOADED_PREVIEW.try_lock() {
            Ok(m) => m.clone(),
            Err(e) => anyhow::bail!("Cannot lock mutex: {e}"),
        }
    };

    if li.data.is_empty() || li.hash.ne(&calc_path_hash(&path)) {
        li = load_preview(path, side)?;
    }

    let end = offset
        .checked_add(count)
        .ok_or_else(|| anyhow!("Add overflow"))?;

    let result = li
        .data
        .get(offset as usize..end as usize)
        .ok_or_else(|| anyhow!("Bad range"))?
        .to_vec();

    Ok(result)
}

/// Load preview into mutex
fn load_preview(path: PathBuf, side: u16) -> Result<Arc<LoadedImage>> {
    let image = image::open(&path)?;
    let mut resized_image = fit_into_square(image, side);

    apply_rounded_mask(&mut resized_image, Rgb([0, 0, 0]));

    // Create vec with two byte per pixel
    let mut rgb565 = Vec::with_capacity((side * side * 2) as usize);

    for pixel in resized_image.pixels() {
        let [r, g, b] = pixel.0;

        let pixel_rgb565: u16 = ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3);
        rgb565.extend_from_slice(&pixel_rgb565.to_be_bytes());
    }

    let arc = Arc::new(LoadedImage {
        data: rgb565,
        hash: calc_path_hash(&path),
    });

    {
        match LOADED_PREVIEW.try_lock() {
            Ok(mut m) => *m = arc.clone(),
            Err(e) => error!("Cannot lock mutex: {e}"),
        }
    }

    Ok(arc)
}

/// Fits provided image to a square with side x
fn fit_into_square(image: DynamicImage, size: u16) -> RgbImage {
    let (width, height) = image.dimensions();

    let scale = (size as f32 / width as f32).min(size as f32 / height as f32);

    let new_width = (width as f32 * scale).round() as u16;
    let new_height = (height as f32 * scale).round() as u16;

    let resized = image.resize_exact(new_width as u32, new_height as u32, FilterType::Lanczos3);

    let mut canvas = RgbImage::from_pixel(size as u32, size as u32, Rgb([16, 16, 19]));

    let x = (size - new_width) / 2;
    let y = (size - new_height) / 2;

    let resized = resized.to_rgb8();
    image::imageops::overlay(&mut canvas, &resized, x.into(), y.into());

    canvas
}

const R: usize = 5;

const MASK: [[u8; R]; R] = [
    [0, 0, 80, 220, 255],
    [0, 150, 255, 255, 255],
    [80, 255, 255, 255, 255],
    [220, 255, 255, 255, 255],
    [255, 255, 255, 255, 255],
];

/// Apply all rounded corners
fn apply_rounded_mask(img: &mut RgbImage, bg: Rgb<u8>) {
    let w = img.width() as usize;
    let h = img.height() as usize;

    for y in 0..R.min(h) {
        for x in 0..R.min(w) {
            apply_pixel(img, x, y, MASK[y][x], bg);
            apply_pixel(img, w - 1 - x, y, MASK[y][x], bg);
            apply_pixel(img, x, h - 1 - y, MASK[y][x], bg);
            apply_pixel(img, w - 1 - x, h - 1 - y, MASK[y][x], bg);
        }
    }
}
fn apply_pixel(img: &mut RgbImage, x: usize, y: usize, alpha: u8, bg: Rgb<u8>) {
    if alpha == 255 {
        return;
    }

    let pixel = img.get_pixel_mut(x as u32, y as u32);

    let a = alpha as u16;
    let inv = 255 - a;

    pixel.0[0] = ((pixel.0[0] as u16 * a + bg.0[0] as u16 * inv) / 255) as u8;
    pixel.0[1] = ((pixel.0[1] as u16 * a + bg.0[1] as u16 * inv) / 255) as u8;
    pixel.0[2] = ((pixel.0[2] as u16 * a + bg.0[2] as u16 * inv) / 255) as u8;
}
