use std::{path::PathBuf, process::ExitCode};

use devup_mcp_visual::{CompareOptions, compare_png};

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(passed) => {
            if passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

fn run(arguments: Vec<String>) -> Result<bool, String> {
    if arguments.first().map(String::as_str) != Some("compare") {
        return Err("usage: devup-mcp-visual compare --reference <png> --actual <png> [--diff <png>] [--channel-tolerance <0..255>] [--max-changed-ratio <0..1>]".to_owned());
    }
    let mut reference = None;
    let mut actual = None;
    let mut options = CompareOptions::default();
    let mut index = 1;
    while index < arguments.len() {
        let option = arguments[index].as_str();
        let value = arguments
            .get(index + 1)
            .ok_or_else(|| format!("{option} 값이 필요합니다."))?;
        match option {
            "--reference" => reference = Some(PathBuf::from(value)),
            "--actual" => actual = Some(PathBuf::from(value)),
            "--diff" => options.diff_path = Some(PathBuf::from(value)),
            "--channel-tolerance" => {
                options.channel_tolerance = value
                    .parse()
                    .map_err(|_| "channel tolerance가 올바르지 않습니다.".to_owned())?;
            }
            "--max-changed-ratio" => {
                options.max_changed_ratio = value
                    .parse()
                    .map_err(|_| "max changed ratio가 올바르지 않습니다.".to_owned())?;
            }
            _ => return Err(format!("알 수 없는 option입니다: {option}")),
        }
        index += 2;
    }
    let report = compare_png(
        reference.ok_or_else(|| "--reference가 필요합니다.".to_owned())?,
        actual.ok_or_else(|| "--actual이 필요합니다.".to_owned())?,
        &options,
    )
    .map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|error| error.to_string())?
    );
    Ok(report.passed())
}
