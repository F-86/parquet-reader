use std::{env, fs, path::PathBuf};

use clap::Parser;

use crate::error::{AppError, Result};

#[derive(Debug, Parser)]
#[command(version, about = "A terminal Parquet viewer")]
pub struct CliArgs {
    /// Optional Parquet file to open on startup.
    pub file_path: Option<PathBuf>,

    /// Number of rows to read for the first page.
    #[arg(long, default_value_t = 50)]
    pub page_size: usize,

    /// Directory for CSV page exports. Defaults to system temp dir.
    #[arg(long)]
    pub export_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub initial_file_path: Option<PathBuf>,
    pub page_size: usize,
    pub root_directory: PathBuf,
    pub export_dir: PathBuf,
}

impl CliArgs {
    pub fn into_config(self) -> Result<AppConfig> {
        let root_directory = env::current_dir()?;
        let initial_file_path = self
            .file_path
            .map(|path| validate_initial_file(path, &root_directory))
            .transpose()?;

        Ok(AppConfig {
            initial_file_path,
            page_size: self.page_size.max(1),
            root_directory,
            export_dir: self.export_dir.unwrap_or_else(std::env::temp_dir),
        })
    }
}

fn validate_initial_file(path: PathBuf, root_directory: &std::path::Path) -> Result<PathBuf> {
    if !path.exists() {
        return Err(AppError::PathNotFound(path));
    }

    let metadata = fs::metadata(&path).map_err(|source| AppError::FileMetadata {
        path: path.clone(),
        source,
    })?;

    if !metadata.is_file() {
        return Err(AppError::NotAFile(path));
    }

    Ok(if path.is_absolute() {
        path
    } else {
        root_directory.join(path)
    })
}
