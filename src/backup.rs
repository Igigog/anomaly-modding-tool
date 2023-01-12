use anyhow::{bail, Context, Result};
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

pub struct FileTransaction<'a> {
    files: Box<dyn AsRef<Path> + 'a>,
}

pub struct SafeTransaction<'a, 'b> {
    transaction: &'a FileTransaction<'a>,
    backup_dir: &'b Path,
    root_dir: &'b Path,
}

impl<'a, 'b> FileTransaction<'a> {
    pub fn new(path: impl AsRef<Path> + 'a) -> Result<FileTransaction<'a>> {
        let p = path.as_ref();
        if !p.is_dir() {
            bail!("Path is not a directory")
        }

        for p in walkdir::WalkDir::new(p) {
            p?;
        }

        Ok(Self {
            files: Box::new(path),
        })
    }

    pub fn relative_file_paths(&self) -> Vec<PathBuf> {
        walkdir::WalkDir::new(self.files.as_ref())
            .into_iter()
            .map(|r| r.expect("Checked for errors in :new()"))
            .filter(|e| e.path().is_file())
            .map(|p| p.into_path())
            .map(|p| p.strip_prefix(self.files.as_ref()).unwrap().to_owned())
            .collect()
    }

    fn check_backup_dir(path: &Path) -> Result<()> {
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
            std::fs::create_dir_all(path)?;
        }

        let tmp_path = path.join("test_writable.txt");
        std::fs::File::create(&tmp_path).or_else(|_| bail!("Directory is not writable"))?;
        std::fs::remove_file(&tmp_path).or_else(|_| bail!("Directory is not writable"))?;

        Ok(())
    }

    fn backup(
        &'a self,
        root_dir: &'b Path,
        backup_dir: &'b Path,
    ) -> Result<SafeTransaction<'a, 'b>> {
        Self::check_backup_dir(backup_dir)?;

        for path in self.relative_file_paths() {
            let root_path = root_dir.join(&path);
            let backup_path = backup_dir.join(&path);
            match std::fs::File::open(&root_path) {
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| root_path.display().to_string()),
                Ok(mut f) => {
                    std::fs::create_dir_all(backup_path.parent().unwrap())?;
                    let mut backup = std::fs::File::create(backup_path)?;
                    std::io::copy(&mut f, &mut backup)?;
                }
            }
        }

        Ok(SafeTransaction {
            transaction: self,
            backup_dir,
            root_dir,
        })
    }

    fn run(&self, root_dir: &Path) -> Result<()> {
        let mut opt = fs_extra::dir::CopyOptions::new();
        opt.overwrite = true;
        opt.content_only = true;
        opt.copy_inside = true;
        fs_extra::dir::copy(self.files.as_ref(), root_dir, &opt)?;

        Ok(())
    }
}

impl SafeTransaction<'_, '_> {
    pub fn run(&self) -> Result<()> {
        let done = self.transaction.run(self.root_dir);
        if let Err(r) = done {
            match self.reverse() {
                Ok(_) => bail!("Fail, but reversed successfully: {}", r),
                Err(e) => bail!("Fail: {}, and you're fucked: {}", r, e),
            }
        }
        Ok(())
    }

    pub fn reverse(&self) -> Result<()> {
        let files = self.transaction.relative_file_paths();

        for path in files {
            match std::fs::remove_file(self.root_dir.join(path)) {
                Ok(()) => {}
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                Err(e) => bail!("Can't remove new files: {}", e),
            };
        }

        let mut opt = fs_extra::dir::CopyOptions::new();
        opt.overwrite = true;
        opt.content_only = true;
        opt.copy_inside = true;
        fs_extra::dir::copy(self.backup_dir, self.root_dir, &opt)?;

        Ok(())
    }
}

impl Drop for SafeTransaction<'_, '_> {
    fn drop(&mut self) {
        std::fs::remove_dir_all(self.backup_dir)
            .unwrap_or_else(|_| println!("Can't delete the backup"));
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use tempfile::tempdir;

    use crate::backup::FileTransaction;

    #[test]
    fn relative_paths() {
        let tmpdir = tempdir().unwrap();
        std::fs::create_dir(tmpdir.path().join("dir")).unwrap();
        std::fs::create_dir_all(tmpdir.path().join("dir2/dir2")).unwrap();

        std::fs::File::create(tmpdir.path().join("config.rs")).unwrap();
        std::fs::File::create(tmpdir.path().join("uwu.rs")).unwrap();
        std::fs::File::create(tmpdir.path().join("dir/config.rs")).unwrap();
        std::fs::File::create(tmpdir.path().join("dir2/config.rs")).unwrap();
        std::fs::File::create(tmpdir.path().join("dir2/dir2/config.rs")).unwrap();

        let tr = FileTransaction::new(tmpdir.path()).unwrap();

        let paths = tr.relative_file_paths();
        let expected = [
            "config.rs",
            "uwu.rs",
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

    #[test]
    fn backup() {
        let tmpdir = tempdir().unwrap();
        let backup_dir = tempdir().unwrap();
        let backup_path = backup_dir.path();
        std::fs::create_dir(tmpdir.path().join("resources")).unwrap();
        std::fs::File::create(tmpdir.path().join("resources/ModOrganizer.ini")).unwrap();
        std::fs::File::create(tmpdir.path().join("resources/nxmhandler.ini")).unwrap();

        let cwd = std::env::current_dir().unwrap();
        let tr = FileTransaction::new(tmpdir.path()).unwrap();

        let backup = tr.backup(&cwd, backup_path).unwrap();

        assert!(
            File::open(backup_path.join("resources/ModOrganizer.ini"))
                .unwrap()
                .metadata()
                .unwrap()
                .len()
                == File::open(backup_path.join("resources/ModOrganizer.ini"))
                    .unwrap()
                    .metadata()
                    .unwrap()
                    .len()
        );
        assert!(
            File::open(backup_path.join("resources/nxmhandler.ini"))
                .unwrap()
                .metadata()
                .unwrap()
                .len()
                == File::open(cwd.join("resources/nxmhandler.ini"))
                    .unwrap()
                    .metadata()
                    .unwrap()
                    .len()
        );

        drop(backup);

        assert!(!backup_path.exists());
    }
}
