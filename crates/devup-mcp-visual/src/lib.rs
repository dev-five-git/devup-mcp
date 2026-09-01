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
            Self::Image(error) => write!(formatter, "PNG를 읽거나 쓸 수 없습니다: {error}"),
            Self::InvalidThreshold => {
                formatter.write_str("max_changed_ratio는 0 이상 1 이하여야 합니다.")
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
    let reference = image::open(reference_path)?.into_rgba8();
    let actual = image::open(actual_path)?.into_rgba8();
    let reference_dimensions = [reference.width(), reference.height()];
    let actual_dimensions = [actual.width(), actual.height()];
    if reference_dimensions != actual_dimensions {
        return Ok(VisualReport {
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
        });
    }

    let mut changed_pixels = 0_u64;
    let mut max_channel_delta = 0_u8;
    let mut diff = options.diff_path.as_ref().map(|_| {
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
    let diff_path = if let (Some(path), Some(diff)) = (&options.diff_path, diff) {
        diff.save_with_format(path, image::ImageFormat::Png)?;
        Some(path.to_string_lossy().into_owned())
    } else {
        None
    };
    Ok(VisualReport {
        status,
        reference_dimensions,
        actual_dimensions,
        changed_pixels,
        total_pixels,
        changed_ratio,
        max_changed_ratio: options.max_changed_ratio,
        channel_tolerance: options.channel_tolerance,
        max_channel_delta,
        diff_path,
    })
}
