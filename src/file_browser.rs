use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileEntryKind {
    Directory,
    ParquetFile,
    OtherFile,
}

#[derive(Debug, Clone)]
pub struct VisibleFileEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: FileEntryKind,
    pub depth: usize,
    pub expanded: bool,
}

#[derive(Debug, Clone)]
pub struct FileSidebar {
    pub root_dir: PathBuf,
    pub entries: Vec<VisibleFileEntry>,
    pub selected: usize,
    pub focused: bool,
    expanded_dirs: HashSet<PathBuf>,
}

impl FileSidebar {
    pub fn new(root_dir: PathBuf) -> Result<Self> {
        let mut expanded_dirs = HashSet::new();
        expanded_dirs.insert(canonicalize_existing(&root_dir)?);
        let mut sidebar = Self {
            root_dir,
            entries: Vec::new(),
            selected: 0,
            focused: false,
            expanded_dirs,
        };
        sidebar.refresh()?;
        Ok(sidebar)
    }

    pub fn refresh(&mut self) -> Result<()> {
        let root = canonicalize_existing(&self.root_dir)?;
        let mut entries = Vec::new();
        append_children(&root, &root, 0, &self.expanded_dirs, &mut entries)?;
        self.entries = entries;
        self.selected = self.selected.min(self.entries.len().saturating_sub(1));
        Ok(())
    }

    pub fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn select_next(&mut self) {
        if self.selected + 1 < self.entries.len() {
            self.selected += 1;
        }
    }

    pub fn select(&mut self, index: usize) {
        if index < self.entries.len() {
            self.selected = index;
        }
    }

    pub fn selected_entry(&self) -> Option<&VisibleFileEntry> {
        self.entries.get(self.selected)
    }

    pub fn toggle_directory(&mut self, path: &Path) -> Result<()> {
        let canonical_root = canonicalize_existing(&self.root_dir)?;
        let canonical_path = canonicalize_existing(path)?;

        if !canonical_path.starts_with(&canonical_root) {
            return Err(AppError::OutsideRoot(path.to_path_buf()));
        }

        if canonical_path == canonical_root {
            self.expanded_dirs.insert(canonical_path);
        } else if !self.expanded_dirs.insert(canonical_path.clone()) {
            self.expanded_dirs.remove(&canonical_path);
        }
        self.refresh()
    }
}

fn append_children(
    root: &Path,
    directory: &Path,
    depth: usize,
    expanded_dirs: &HashSet<PathBuf>,
    output: &mut Vec<VisibleFileEntry>,
) -> Result<()> {
    let mut children = read_directory_entries(directory)?;
    children.sort_by(|a, b| {
        kind_order(a.kind)
            .cmp(&kind_order(b.kind))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    for child in children {
        let is_expanded =
            child.kind == FileEntryKind::Directory && expanded_dirs.contains(&child.path);
        let child_path = child.path.clone();
        output.push(VisibleFileEntry {
            expanded: is_expanded,
            depth,
            ..child
        });
        if is_expanded && child_path.starts_with(root) {
            append_children(root, &child_path, depth + 1, expanded_dirs, output)?;
        }
    }

    Ok(())
}

fn read_directory_entries(directory: &Path) -> Result<Vec<VisibleFileEntry>> {
    let read_dir = fs::read_dir(directory).map_err(|source| AppError::ReadDirectory {
        path: directory.to_path_buf(),
        source,
    })?;

    let mut entries = Vec::new();
    for entry in read_dir {
        let entry = entry.map_err(|source| AppError::ReadDirectory {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let metadata = entry.metadata().map_err(|source| AppError::FileMetadata {
            path: path.clone(),
            source,
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = if metadata.is_dir() {
            FileEntryKind::Directory
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("parquet"))
        {
            FileEntryKind::ParquetFile
        } else {
            FileEntryKind::OtherFile
        };

        entries.push(VisibleFileEntry {
            name,
            path: canonicalize_existing(&path)?,
            kind,
            depth: 0,
            expanded: false,
        });
    }

    Ok(entries)
}

fn canonicalize_existing(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .map_err(|source| AppError::FileMetadata {
            path: path.to_path_buf(),
            source,
        })
}

fn kind_order(kind: FileEntryKind) -> u8 {
    match kind {
        FileEntryKind::Directory => 0,
        FileEntryKind::ParquetFile => 1,
        FileEntryKind::OtherFile => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_dir(path: &Path, name: &str) -> PathBuf {
        let dir = path.join(name);
        fs::create_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn cannot_navigate_outside_root() {
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().to_path_buf();
        let sub = make_dir(&root_path, "sub");
        make_dir(&sub, "nested");

        let mut sidebar = FileSidebar::new(root_path.clone()).unwrap();
        assert!(sidebar.toggle_directory(&sub).is_ok());

        let outside = root_path
            .parent()
            .map(|parent| parent.to_path_buf())
            .unwrap_or_else(|| root_path.clone());
        assert!(sidebar.toggle_directory(&outside).is_err());

        let sibling = root_path.join("..");
        assert!(sidebar.toggle_directory(&sibling).is_err());

        let absolute = std::env::temp_dir().join("definitely_outside_root");
        assert!(sidebar.toggle_directory(&absolute).is_err());
    }

    #[test]
    fn toggling_directories_builds_visible_tree() {
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().to_path_buf();
        let sub = make_dir(&root_path, "sub");
        make_dir(&sub, "nested");

        let mut sidebar = FileSidebar::new(root_path.clone()).unwrap();
        assert_eq!(sidebar.entries.len(), 1);

        sidebar.toggle_directory(&sub).unwrap();
        assert_eq!(sidebar.entries.len(), 2);

        sidebar.toggle_directory(&sub).unwrap();
        assert_eq!(sidebar.entries.len(), 1);
    }
}
