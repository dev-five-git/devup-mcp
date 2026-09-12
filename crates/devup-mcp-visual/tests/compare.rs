use std::{path::PathBuf, time::SystemTime};

use devup_mcp_visual::{CompareOptions, VisualStatus, compare_png};
use image::{ImageBuffer, Rgba};

/// One directory per test. The two tests here run on two threads of one
/// process, and a clock read on each is the same value often enough that
/// they shared a directory and each other's PNGs - a 4x2 reference next to
/// a 3x2 actual, reported as `InvalidDimensions` by whichever test lost.
fn temp_dir(test: &str) -> anyhow::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "devup-mcp-visual-{test}-{}-{}",
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
    let root = temp_dir("exact-and-changed")?;
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
    let root = temp_dir("tolerance-and-dimensions")?;
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

#[test]
fn memory_comparison_returns_red_diff_without_writing_a_path() -> anyhow::Result<()> {
    use devup_mcp_visual::compare_png_bytes;
    let mut reference = std::io::Cursor::new(Vec::new());
    let mut actual = std::io::Cursor::new(Vec::new());
    let white = ImageBuffer::from_pixel(2, 1, Rgba([255_u8; 4]));
    let mut changed = white.clone();
    changed.put_pixel(1, 0, Rgba([0, 0, 0, 255]));
    white.write_to(&mut reference, image::ImageFormat::Png)?;
    changed.write_to(&mut actual, image::ImageFormat::Png)?;
    let (report, diff) = compare_png_bytes(
        reference.get_ref(),
        actual.get_ref(),
        &CompareOptions {
            max_changed_ratio: 0.5,
            diff_path: Some(PathBuf::from("nonexistent-directory/must-not-write.png")),
            ..CompareOptions::default()
        },
        true,
    )?;
    assert_eq!(report.changed_ratio, 0.5);
    assert!(report.passed());
    assert!(report.diff_path.is_none());
    let decoded =
        image::load_from_memory_with_format(&diff.unwrap(), image::ImageFormat::Png)?.into_rgba8();
    assert_eq!(decoded.get_pixel(0, 0).0, [0, 0, 0, 0]);
    assert_eq!(decoded.get_pixel(1, 0).0, [255, 0, 0, 255]);
    Ok(())
}

#[test]
fn memory_comparison_refuses_invalid_png_and_nonfinite_threshold() {
    use devup_mcp_visual::{VisualError, compare_png_bytes};
    assert!(matches!(
        compare_png_bytes(b"not PNG", b"not PNG", &CompareOptions::default(), false),
        Err(VisualError::Image(_))
    ));
    assert!(matches!(
        compare_png_bytes(
            &[],
            &[],
            &CompareOptions {
                max_changed_ratio: f64::NAN,
                ..CompareOptions::default()
            },
            false
        ),
        Err(VisualError::InvalidThreshold)
    ));
}
