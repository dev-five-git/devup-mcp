use std::path::{Path, PathBuf};

use image::{ImageBuffer, ImageError, Rgba};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq)]
pub struct CompareOptions {
    pub channel_tolerance: u8,
    pub max_changed_ratio: f64,
    pub diff_path: Option<PathBuf>,
}

impl Default for CompareOptions {
    fn default() -> Self {
        Self {
            channel_tolerance: 0,
            max_changed_ratio: 0.005,
            diff_path: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VisualStatus {
    Exact,
    WithinThreshold,
    Mismatch,
    InvalidDimensions,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualReport {
    pub status: VisualStatus,
    pub reference_dimensions: [u32; 2],
    pub actual_dimensions: [u32; 2],
    pub changed_pixels: u64,
    pub total_pixels: u64,
    pub changed_ratio: f64,
    pub max_changed_ratio: f64,
    pub channel_tolerance: u8,
    pub max_channel_delta: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_path: Option<String>,
}

impl VisualReport {
    pub fn passed(&self) -> bool {
        matches!(
            self.status,
            VisualStatus::Exact | VisualStatus::WithinThreshold
        )
    }
}

#[derive(Debug)]
pub enum VisualError {
    Image(ImageError),
    InvalidThreshold,
}

impl std::fmt::Display for VisualError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Image(error) => write!(formatter, "Could not read or write the PNG: {error}"),
            Self::InvalidThreshold => {
                formatter.write_str("max_changed_ratio must be between 0 and 1 inclusive.")
            }
        }
    }
}

impl std::error::Error for VisualError {}

impl From<ImageError> for VisualError {
    fn from(value: ImageError) -> Self {
        Self::Image(value)
    }
}

pub fn compare_png(
    reference_path: impl AsRef<Path>,
    actual_path: impl AsRef<Path>,
    options: &CompareOptions,
) -> Result<VisualReport, VisualError> {
    if !(0.0..=1.0).contains(&options.max_changed_ratio) {
        return Err(VisualError::InvalidThreshold);
    }
    let reference = std::fs::read(reference_path).map_err(ImageError::IoError)?;
    let actual = std::fs::read(actual_path).map_err(ImageError::IoError)?;
    let (mut report, diff) =
        compare_png_bytes(&reference, &actual, options, options.diff_path.is_some())?;
    if let (Some(path), Some(bytes)) = (&options.diff_path, diff) {
        std::fs::write(path, bytes).map_err(ImageError::IoError)?;
        report.diff_path = Some(path.to_string_lossy().into_owned());
    }
    Ok(report)
}

/// Compare PNGs without disk writes. The optional diff uses opaque red changed
/// pixels and transparent unchanged pixels, exactly as the CLI does.
/// Inputs are limited to 16 MiB compressed, 8192 pixels per axis, and 64 MiB RGBA.
pub fn compare_png_bytes(
    reference: &[u8],
    actual: &[u8],
    options: &CompareOptions,
    include_diff: bool,
) -> Result<(VisualReport, Option<Vec<u8>>), VisualError> {
    if !(0.0..=1.0).contains(&options.max_changed_ratio) {
        return Err(VisualError::InvalidThreshold);
    }
    let reference = decode_png(reference)?;
    let actual = decode_png(actual)?;
    let reference_dimensions = [reference.width(), reference.height()];
    let actual_dimensions = [actual.width(), actual.height()];
    if reference_dimensions != actual_dimensions {
        return Ok((
            VisualReport {
                status: VisualStatus::InvalidDimensions,
                reference_dimensions,
                actual_dimensions,
                changed_pixels: 0,
                total_pixels: 0,
                changed_ratio: 1.0,
                max_changed_ratio: options.max_changed_ratio,
                channel_tolerance: options.channel_tolerance,
                max_channel_delta: 0,
                diff_path: None,
            },
            None,
        ));
    }

    let mut changed_pixels = 0_u64;
    let mut max_channel_delta = 0_u8;
    let mut diff = include_diff.then(|| {
        ImageBuffer::from_pixel(
            reference.width(),
            reference.height(),
            Rgba::<u8>([0, 0, 0, 0]),
        )
    });
    for (index, (reference_pixel, actual_pixel)) in
        reference.pixels().zip(actual.pixels()).enumerate()
    {
        let delta = reference_pixel
            .0
            .iter()
            .zip(actual_pixel.0)
            .map(|(reference, actual)| reference.abs_diff(actual))
            .max()
            .unwrap_or(0);
        max_channel_delta = max_channel_delta.max(delta);
        if delta <= options.channel_tolerance {
            continue;
        }
        changed_pixels += 1;
        if let Some(diff) = &mut diff {
            let x = index as u32 % reference.width();
            let y = index as u32 / reference.width();
            diff.put_pixel(x, y, Rgba::<u8>([255, 0, 0, 255]));
        }
    }
    let total_pixels = u64::from(reference.width()) * u64::from(reference.height());
    let changed_ratio = if total_pixels == 0 {
        0.0
    } else {
        changed_pixels as f64 / total_pixels as f64
    };
    let status = if changed_pixels == 0 {
        VisualStatus::Exact
    } else if changed_ratio <= options.max_changed_ratio {
        VisualStatus::WithinThreshold
    } else {
        VisualStatus::Mismatch
    };
    let diff_png = diff
        .map(|diff| {
            let mut bytes = std::io::Cursor::new(Vec::new());
            diff.write_to(&mut bytes, image::ImageFormat::Png)?;
            Ok::<_, ImageError>(bytes.into_inner())
        })
        .transpose()?;
    Ok((
        VisualReport {
            status,
            reference_dimensions,
            actual_dimensions,
            changed_pixels,
            total_pixels,
            changed_ratio,
            max_changed_ratio: options.max_changed_ratio,
            channel_tolerance: options.channel_tolerance,
            max_channel_delta,
            diff_path: None,
        },
        diff_png,
    ))
}

fn decode_png(bytes: &[u8]) -> Result<image::RgbaImage, VisualError> {
    use image::{ImageDecoder, Limits, codecs::png::PngDecoder};
    let invalid = || {
        ImageError::Limits(image::error::LimitError::from_kind(
            image::error::LimitErrorKind::DimensionError,
        ))
    };
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(invalid().into());
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    let decoder = PngDecoder::with_limits(std::io::Cursor::new(bytes), limits)?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) * 4 > 64 * 1024 * 1024 {
        return Err(invalid().into());
    }
    Ok(image::DynamicImage::from_decoder(decoder)?.into_rgba8())
}
