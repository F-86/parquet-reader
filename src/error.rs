use std::{io, path::PathBuf};

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("path does not exist: {0}")]
    PathNotFound(PathBuf),

    #[error("path is not a file: {0}")]
    NotAFile(PathBuf),

    #[error("path is outside the workspace root: {0}")]
    OutsideRoot(PathBuf),

    #[error("failed to read directory {path}: {source}")]
    ReadDirectory { path: PathBuf, source: io::Error },

    #[error("failed to read file metadata {path}: {source}")]
    FileMetadata { path: PathBuf, source: io::Error },

    #[error("failed to open Parquet file {path}: {source}")]
    OpenParquet {
        path: PathBuf,
        source: parquet::errors::ParquetError,
    },

    #[error("failed to read Parquet data: {0}")]
    ReadParquet(#[from] parquet::errors::ParquetError),

    #[error("failed to read Arrow data: {0}")]
    ReadArrow(#[from] arrow_schema::ArrowError),

    #[error("invalid filter: {0}")]
    InvalidFilter(String),

    #[error("terminal error: {0}")]
    Terminal(#[from] io::Error),
}
