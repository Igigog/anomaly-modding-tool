use anyhow::{bail, Result};
use std::{
    collections::HashMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

#[derive(Default)]
pub struct FileTransaction {
    files: Directory,
}

#[derive(Default)]
struct Directory {
    dirs: HashMap<OsString, Directory>,
    files: HashMap<OsString, File>,
}

struct File {
    data: Box<dyn std::io::Read>,
}

struct SafeTransaction {
    transaction: FileTransaction,
    backup_dir: BackupDir,
    root_dir: PathBuf,
}

struct BackupDir(PathBuf);

impl Directory {
    pub fn get_relative_paths(&self) -> Vec<PathBuf> {
        let files = self.files.keys().map(PathBuf::from);
        let subdirectories = self.dirs.keys().into_iter().map(PathBuf::from);
        let subdirectory_files = self.dirs.iter().flat_map(|(k, d)| {
            d.get_relative_paths()
                .into_iter()
                .map(move |p| PathBuf::from(k).join(p))
        });

        files
            .chain(subdirectories)
            .chain(subdirectory_files)
            .collect()
    }
}

impl FileTransaction {
    pub fn new() -> FileTransaction {
        Self::default()
    }

    pub fn get_relative_paths(&self) -> Vec<PathBuf> {
        self.files.get_relative_paths()
    }

    pub fn run_backup(self, root_dir: PathBuf, backup_dir: BackupDir) -> Result<SafeTransaction> {
        let mut current_dir = &root_dir;
        let mut checking_dir = &self.files;
        let mut to_backup: Vec<PathBuf> = Vec::new();
        for name in checking_dir.files.keys() {
            let file = current_dir.join(name);
            if file.exists() {
                to_backup.push(file);
            }
        }

        Ok(SafeTransaction {
            transaction: self,
            backup_dir,
            root_dir,
        })
    }
}

impl BackupDir {
    pub fn new(path: PathBuf) -> Result<BackupDir> {
        if path.is_file() {
            anyhow::bail!("Backup path is a file");
        }

        if path.is_dir() {
            let mut entries = path.read_dir()?;
            if entries.next().is_some() {
                anyhow::bail!("Backup directory is not empty");
            }
        }

        if !path.exists() {
            std::fs::create_dir_all(&path)?;
        }

        let tmp_path = path.join("test_writable.txt");
        std::fs::File::create(&tmp_path).or_else(|_| bail!("Directory is not writable"))?;
        std::fs::remove_file(&tmp_path).or_else(|_| bail!("Directory is not writable"))?;

        Ok(Self(path))
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap};

    use super::{Directory, File};

    static NOTHING: [u8; 0] = [0; 0];

    #[test]
    fn relative_paths() {
        let mut dir = Directory {
            dirs: HashMap::new(),
            files: HashMap::new(),
        };
        dir.files.insert(
            "config.rs".to_owned().into(),
            File {
                data: Box::new(NOTHING.as_slice()),
            },
        );
        dir.files.insert(
            "uwu.rs".to_owned().into(),
            File {
                data: Box::new(NOTHING.as_slice()),
            },
        );

        let mut subdir = Directory {
            dirs: HashMap::new(),
            files: HashMap::new(),
        };
        subdir.files.insert(
            "config.rs".to_owned().into(),
            File {
                data: Box::new(NOTHING.as_slice()),
            },
        );
        dir.dirs.insert("dir".to_owned().into(), subdir);

        let mut subdir2 = Directory {
            dirs: HashMap::new(),
            files: HashMap::new(),
        };
        subdir2.files.insert(
            "config.rs".to_owned().into(),
            File {
                data: Box::new(NOTHING.as_slice()),
            },
        );

        let mut subdir3 = Directory {
            dirs: HashMap::new(),
            files: HashMap::new(),
        };
        subdir3.files.insert(
            "config.rs".to_owned().into(),
            File {
                data: Box::new(NOTHING.as_slice()),
            },
        );
        subdir2.dirs.insert("dir2".to_owned().into(), subdir3);
        dir.dirs.insert("dir2".to_owned().into(), subdir2);

        let paths = dir.get_relative_paths();
        let expected = [
            "config.rs",
            "uwu.rs",
            "dir",
            "dir2",
            "dir2/dir2",
            "dir/config.rs",
            "dir2/config.rs",
            "dir2/dir2/config.rs",
        ];
        assert_eq!(paths.len(), expected.len());

        for x in expected {
            let p = std::path::Path::new(x);
            assert!(paths.iter().any(|e| e == p))
        }
    }
}
