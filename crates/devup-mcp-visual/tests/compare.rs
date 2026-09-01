use std::{path::PathBuf, time::SystemTime};

use devup_mcp_visual::{CompareOptions, VisualStatus, compare_png};
use image::{ImageBuffer, Rgba};

fn temp_dir() -> anyhow::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "devup-mcp-visual-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_nanos()
    ));
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

fn write_png(path: &std::path::Path, pixels: &[[u8; 4]], width: u32) -> anyhow::Result<()> {
    let height = pixels.len() as u32 / width;
    let image = ImageBuffer::<Rgba<u8>, _>::from_raw(
        width,
        height,
        pixels.iter().flatten().copied().collect::<Vec<_>>(),
    )
    .expect("fixture dimensions");
    image.save_with_format(path, image::ImageFormat::Png)?;
    Ok(())
}

#[test]
fn exact_and_changed_pngs_report_deterministic_metrics_and_diff() -> anyhow::Result<()> {
    let root = temp_dir()?;
    let reference = root.join("reference.png");
    let actual = root.join("actual.png");
    let diff = root.join("diff.png");
    let white = [255, 255, 255, 255];
    write_png(&reference, &[white; 8], 4)?;
    write_png(&actual, &[white; 8], 4)?;
    let exact = compare_png(&reference, &actual, &CompareOptions::default())?;
    assert_eq!(exact.status, VisualStatus::Exact);
    assert_eq!(exact.changed_pixels, 0);

    let mut changed = [white; 8];
    changed[1] = [0, 0, 0, 255];
    changed[2] = [0, 0, 0, 255];
    write_png(&actual, &changed, 4)?;
    let report = compare_png(
        &reference,
        &actual,
        &CompareOptions {
            diff_path: Some(diff.clone()),
            ..CompareOptions::default()
        },
    )?;
    assert_eq!(report.status, VisualStatus::Mismatch);
    assert_eq!(report.changed_pixels, 2);
    assert_eq!(report.total_pixels, 8);
    assert_eq!(report.changed_ratio, 0.25);
    assert!(diff.exists());
    std::fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn tolerance_and_dimension_mismatch_are_explicit() -> anyhow::Result<()> {
    let root = temp_dir()?;
    let reference = root.join("reference.png");
    let actual = root.join("actual.png");
    write_png(&reference, &[[100, 100, 100, 255]; 4], 2)?;
    write_png(&actual, &[[102, 100, 100, 255]; 4], 2)?;
    let tolerated = compare_png(
        &reference,
        &actual,
        &CompareOptions {
            channel_tolerance: 2,
            ..CompareOptions::default()
        },
    )?;
    assert_eq!(tolerated.status, VisualStatus::Exact);

    write_png(&actual, &[[100, 100, 100, 255]; 6], 3)?;
    let mismatch = compare_png(&reference, &actual, &CompareOptions::default())?;
    assert_eq!(mismatch.status, VisualStatus::InvalidDimensions);
    assert!(!mismatch.passed());
    std::fs::remove_dir_all(root)?;
    Ok(())
}
