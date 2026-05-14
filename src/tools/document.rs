use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{app_error, AppResult};
use crate::tools::types::{Tool, ToolInput, ToolOutput};
use crate::workspace_trust::resolve_workspace_path;

const SUPPORTED_TARGET_FORMATS: &[&str] = &[
    "markdown",
    "gfm",
    "commonmark",
    "html",
    "rst",
    "latex",
    "docx",
    "odt",
    "epub",
    "plain",
    "asciidoc",
];

pub struct PandocConvertTool;
pub struct ImageOcrTool;

impl Tool for PandocConvertTool {
    fn name(&self) -> &str {
        "pandoc_convert"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let source_path = required_nonempty(&input, "source_path", "pandoc_convert")?;
        let target_format = required_nonempty(&input, "target_format", "pandoc_convert")?
            .trim()
            .to_ascii_lowercase();
        if !SUPPORTED_TARGET_FORMATS.contains(&target_format.as_str()) {
            return Err(app_error(format!(
                "unsupported target_format `{target_format}`. Pick one of: {}",
                SUPPORTED_TARGET_FORMATS.join(", ")
            )));
        }
        let base = workspace_base(&input);
        let source = safe_workspace_path(&base, &source_path, "pandoc_convert source")?;
        if !source.exists() {
            return Err(app_error(format!(
                "pandoc_convert source_path does not exist: {}",
                source.display()
            )));
        }
        if source.is_dir() {
            return Err(app_error(format!(
                "pandoc_convert source_path is a directory: {}",
                source.display()
            )));
        }

        let output_path = input
            .get("output_path")
            .filter(|value| !value.trim().is_empty());
        if output_path.is_none() && format_is_binary(&target_format) {
            return Err(app_error(format!(
                "target_format `{target_format}` is binary; provide an `output_path` to write the converted file"
            )));
        }
        let resolved_output = match output_path {
            Some(path) => {
                let output = safe_workspace_path(&base, path, "pandoc_convert output")?;
                refuse_symlink_components(&output, "pandoc_convert")?;
                if let Ok(metadata) = fs::symlink_metadata(&output) {
                    if metadata.file_type().is_symlink() {
                        return Err(app_error(format!(
                            "pandoc_convert refuses symlink output target: {}",
                            output.display()
                        )));
                    }
                    if metadata.is_dir() {
                        return Err(app_error(format!(
                            "pandoc_convert output_path is a directory: {}",
                            output.display()
                        )));
                    }
                }
                Some(output)
            }
            None => None,
        };

        let mut command = Command::new("pandoc");
        command.arg(&source);
        command.arg("--to").arg(&target_format);
        if let Some(output) = resolved_output.as_ref() {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            command.arg("--output").arg(output);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = command.output().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                app_error(
                    "pandoc_convert: pandoc binary not found on PATH. Install pandoc and retry.",
                )
            } else {
                app_error(format!("failed to launch pandoc: {error}"))
            }
        })?;
        if !output.status.success() {
            return Err(app_error(format!(
                "pandoc failed (exit {:?}): {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }

        if let Some(path) = resolved_output {
            Ok(ToolOutput {
                summary: format!(
                    "Converted {} -> {} via pandoc; wrote {}",
                    source.display(),
                    target_format,
                    path.display()
                ),
            })
        } else {
            Ok(ToolOutput {
                summary: String::from_utf8_lossy(&output.stdout).to_string(),
            })
        }
    }
}

impl Tool for ImageOcrTool {
    fn name(&self) -> &str {
        "image_ocr"
    }

    fn execute(&self, input: ToolInput) -> AppResult<ToolOutput> {
        let raw_path = required_nonempty(&input, "path", "image_ocr")?;
        let base = workspace_base(&input);
        let image = safe_workspace_path(&base, &raw_path, "image_ocr")?;
        if !image.exists() {
            return Err(app_error(format!(
                "image_ocr source path does not exist: {}",
                image.display()
            )));
        }
        if image.is_dir() {
            return Err(app_error(format!(
                "image_ocr source path is a directory: {}",
                image.display()
            )));
        }

        let output = Command::new("tesseract")
            .arg(&image)
            .arg("-")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    app_error(
                        "image_ocr: tesseract binary not found on PATH. Install tesseract and retry.",
                    )
                } else {
                    app_error(format!("failed to launch tesseract: {error}"))
                }
            })?;
        if !output.status.success() {
            return Err(app_error(format!(
                "tesseract failed (exit {:?}): {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(ToolOutput {
            summary: String::from_utf8_lossy(&output.stdout)
                .trim_end()
                .to_string(),
        })
    }
}

fn required_nonempty(input: &ToolInput, key: &str, tool_name: &str) -> AppResult<String> {
    input
        .get(key)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| app_error(format!("{tool_name} requires `{key}`")))
}

fn workspace_base(input: &ToolInput) -> PathBuf {
    input
        .get("cwd")
        .or_else(|| input.get("workspace"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn safe_workspace_path(base: &Path, raw_path: &str, tool_name: &str) -> AppResult<PathBuf> {
    resolve_workspace_path(base, raw_path, tool_name)
}

fn refuse_symlink_components(target: &Path, tool_name: &str) -> AppResult<()> {
    let mut current = PathBuf::new();
    for component in target.components() {
        current.push(component.as_os_str());
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            if metadata.file_type().is_symlink() {
                return Err(app_error(format!(
                    "{tool_name} refuses symlink path component: {}",
                    current.display()
                )));
            }
        }
    }
    Ok(())
}

fn format_is_binary(target_format: &str) -> bool {
    matches!(target_format, "docx" | "odt" | "epub")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "deepseek-document-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn pandoc_convert_rejects_unsupported_target_format() {
        let root = temp_root("format");
        fs::write(root.join("note.md"), "# hi\n").unwrap();
        let error = PandocConvertTool
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("source_path", "note.md")
                    .with_arg("target_format", "definitely-not-real"),
            )
            .unwrap_err();

        assert!(error.to_string().contains("unsupported target_format"));
    }

    #[test]
    fn pandoc_convert_requires_output_for_binary_formats() {
        let root = temp_root("binary");
        fs::write(root.join("note.md"), "# hi\n").unwrap();
        let error = PandocConvertTool
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("source_path", "note.md")
                    .with_arg("target_format", "docx"),
            )
            .unwrap_err();

        assert!(error.to_string().contains("binary"));
        assert!(error.to_string().contains("output_path"));
    }

    #[test]
    fn pandoc_convert_rejects_missing_source_path_before_spawn() {
        let root = temp_root("missing");
        let error = PandocConvertTool
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("source_path", "missing.md")
                    .with_arg("target_format", "html"),
            )
            .unwrap_err();

        assert!(error.to_string().contains("source_path does not exist"));
    }

    #[test]
    fn image_ocr_rejects_missing_path_before_spawn() {
        let root = temp_root("ocr-missing");
        let error = ImageOcrTool
            .execute(
                ToolInput::new()
                    .with_arg("cwd", root.display().to_string())
                    .with_arg("path", "missing.png"),
            )
            .unwrap_err();

        assert!(error.to_string().contains("does not exist"));
    }

    #[test]
    fn binary_format_detection_matches_upstream_set() {
        for format in ["docx", "odt", "epub"] {
            assert!(format_is_binary(format));
        }
        for format in [
            "markdown",
            "gfm",
            "commonmark",
            "html",
            "rst",
            "latex",
            "plain",
            "asciidoc",
        ] {
            assert!(!format_is_binary(format));
        }
    }
}
