use std::{
    fs::OpenOptions,
    io::IoSlice,
    path::{Component, Path, PathBuf},
};

use anyhow::Context;
use tracing::warn;

use crate::{
    storage::{StorageFactoryExt, filesystem::opened_file::OurFileExt},
    torrent_state::{ManagedTorrentShared, TorrentMetadata},
};

use crate::storage::{StorageFactory, TorrentStorage};

use super::opened_file::OpenedFile;

#[derive(Default, Clone, Copy)]
pub struct FilesystemStorageFactory {}

impl StorageFactory for FilesystemStorageFactory {
    type Storage = FilesystemStorage;

    fn create(
        &self,
        shared: &ManagedTorrentShared,
        _metadata: &TorrentMetadata,
    ) -> anyhow::Result<FilesystemStorage> {
        Ok(FilesystemStorage {
            output_folder: shared.options.output_folder.clone(),
            opened_files: Default::default(),
        })
    }

    fn clone_box(&self) -> crate::storage::BoxStorageFactory {
        self.boxed()
    }
}

pub struct FilesystemStorage {
    pub(crate) output_folder: PathBuf,
    pub(crate) opened_files: Vec<OpenedFile>,
}

impl FilesystemStorage {
    fn torrent_path(&self, relative: &Path, create_parents: bool) -> anyhow::Result<PathBuf> {
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            anyhow::bail!("invalid torrent file path")
        }

        if create_parents {
            std::fs::create_dir_all(&self.output_folder)?;
        }
        // The configured output folder itself may be a symlink, but torrent
        // paths below it must not follow links into another directory.
        let mut path = self.output_folder.canonicalize()?;
        for component in relative.parent().into_iter().flat_map(Path::components) {
            let Component::Normal(name) = component else {
                anyhow::bail!("invalid torrent file path")
            };
            path.push(name);
            if create_parents {
                match std::fs::create_dir(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(error.into()),
                }
            }
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                anyhow::bail!("torrent path contains a symlink or non-directory component")
            }
        }
        path.push(relative.file_name().context("invalid torrent file path")?);
        if create_parents
            && std::fs::symlink_metadata(&path)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            anyhow::bail!("torrent file path is a symlink")
        }
        Ok(path)
    }

    #[allow(dead_code)]
    pub(crate) fn take_fs(&self) -> anyhow::Result<Self> {
        Ok(Self {
            opened_files: self
                .opened_files
                .iter()
                .map(|f| f.take_clone())
                .collect::<anyhow::Result<Vec<_>>>()?,
            output_folder: self.output_folder.clone(),
        })
    }
}

impl TorrentStorage for FilesystemStorage {
    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        self.opened_files
            .get(file_id)
            .context("no such file")?
            .lock_read()?
            .pread_exact(offset, buf)
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        let of = self.opened_files.get(file_id).context("no such file")?;
        #[cfg(windows)]
        return of.try_mark_sparse()?.pwrite_all(offset, buf);
        #[cfg(not(windows))]
        return of.lock_read()?.pwrite_all(offset, buf);
    }

    fn pwrite_all_vectored(
        &self,
        file_id: usize,
        offset: u64,
        bufs: [IoSlice<'_>; 2],
    ) -> anyhow::Result<usize> {
        let of = self.opened_files.get(file_id).context("no such file")?;
        #[cfg(windows)]
        return of.try_mark_sparse()?.pwrite_all_vectored(offset, bufs);
        #[cfg(not(windows))]
        return of.lock_read()?.pwrite_all_vectored(offset, bufs);
    }

    fn remove_file(&self, _file_id: usize, filename: &Path) -> anyhow::Result<()> {
        Ok(std::fs::remove_file(self.torrent_path(filename, false)?)?)
    }

    fn ensure_file_length(&self, file_id: usize, len: u64) -> anyhow::Result<()> {
        let f = &self.opened_files.get(file_id).context("no such file")?;
        #[cfg(windows)]
        f.try_mark_sparse()?;
        Ok(f.lock_read()?.set_len(len)?)
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        Ok(Box::new(Self {
            opened_files: self
                .opened_files
                .iter()
                .map(|f| f.take_clone())
                .collect::<anyhow::Result<Vec<_>>>()?,
            output_folder: self.output_folder.clone(),
        }))
    }

    fn remove_directory_if_empty(&self, path: &Path) -> anyhow::Result<()> {
        // Session deletion passes an empty path for a torrent-specific output
        // folder, after removing its files and child directories.
        let path = if path.as_os_str().is_empty() {
            self.output_folder.clone()
        } else {
            self.torrent_path(path, false)?
        };
        if !std::fs::symlink_metadata(&path)?.is_dir() {
            anyhow::bail!("cannot remove dir: {path:?} is not a directory")
        }
        if std::fs::read_dir(&path)?.count() == 0 {
            std::fs::remove_dir(&path).with_context(|| format!("error removing {path:?}"))
        } else {
            warn!("did not remove {path:?} as it was not empty");
            Ok(())
        }
    }

    fn init(
        &mut self,
        shared: &ManagedTorrentShared,
        metadata: &TorrentMetadata,
    ) -> anyhow::Result<()> {
        let mut files = Vec::<OpenedFile>::new();
        for file_details in metadata.file_infos.iter() {
            let relative_path = &file_details.relative_filename;

            if file_details.attrs.padding {
                files.push(OpenedFile::new_dummy());
                continue;
            };
            let full_path = self.torrent_path(relative_path, true)?;
            let mut open_options = OpenOptions::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                open_options.custom_flags(nix::libc::O_NOFOLLOW);
            }
            let f = if shared.options.allow_overwrite {
                open_options
                    .create(true)
                    .truncate(false)
                    .read(true)
                    .write(true)
                    .open(&full_path)
                    .with_context(|| format!("error opening {full_path:?} in read/write mode"))?
            } else {
                // create_new does not seem to work with read(true), so calling this twice.
                open_options
                    .create_new(true)
                    .write(true)
                    .open(&full_path)
                    .with_context(|| {
                        format!(
                            "error creating a new file (because allow_overwrite = false) {:?}",
                            full_path
                        )
                    })?;
                let mut read_write = OpenOptions::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    read_write.custom_flags(nix::libc::O_NOFOLLOW);
                }
                read_write.read(true).write(true).open(&full_path)?
            };
            files.push(OpenedFile::new(full_path.clone(), f));
        }

        self.opened_files = files;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::FilesystemStorage;

    #[test]
    fn torrent_paths_reject_traversal() {
        let root = tempfile::tempdir().unwrap();
        let storage = FilesystemStorage {
            output_folder: root.path().to_path_buf(),
            opened_files: Vec::new(),
        };
        assert!(
            storage
                .torrent_path(Path::new("safe/file.mkv"), true)
                .is_ok()
        );
        assert!(storage.torrent_path(Path::new("../outside"), true).is_err());
        assert!(storage.torrent_path(Path::new("/outside"), true).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn torrent_paths_reject_symlinks_below_output_folder() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let storage = FilesystemStorage {
            output_folder: root.path().to_path_buf(),
            opened_files: Vec::new(),
        };

        symlink(outside.path(), root.path().join("linked_dir")).unwrap();
        assert!(
            storage
                .torrent_path(Path::new("linked_dir/file.mkv"), true)
                .is_err()
        );
        assert!(
            storage
                .torrent_path(Path::new("linked_dir/file.mkv"), false)
                .is_err()
        );
        assert!(!outside.path().join("file.mkv").exists());

        let target = outside.path().join("target.mkv");
        std::fs::write(&target, b"untouched").unwrap();
        symlink(&target, root.path().join("linked_file.mkv")).unwrap();
        assert!(
            storage
                .torrent_path(Path::new("linked_file.mkv"), true)
                .is_err()
        );
        assert_eq!(std::fs::read(target).unwrap(), b"untouched");
    }

    #[cfg(unix)]
    #[test]
    fn deletion_does_not_remove_a_symlinked_output_folder() {
        use std::os::unix::fs::symlink;

        use crate::storage::TorrentStorage;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let linked_root = root.path().join("linked_root");
        symlink(outside.path(), &linked_root).unwrap();
        let storage = FilesystemStorage {
            output_folder: linked_root,
            opened_files: Vec::new(),
        };

        assert!(storage.remove_directory_if_empty(Path::new("")).is_err());
        assert!(outside.path().exists());
    }
}
