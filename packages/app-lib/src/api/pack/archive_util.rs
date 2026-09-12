//! Shared helpers for extracting content from local modpack archives.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use super::detect::decode_zip_entry_name;
use crate::util::io;
use tokio_util::sync::CancellationToken;

const EXTRACTION_SIZE_LIMIT: u64 = 8 * 1024 * 1024 * 1024;

fn archive_error(error: zip::result::ZipError) -> crate::Error {
    crate::ErrorKind::InputError(format!("Modpack archive is invalid: {error}"))
        .into()
}

pub(crate) fn safe_relative_path(value: &str) -> crate::Result<String> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(crate::ErrorKind::InputError(
            "Modpack archive contains an invalid file path".to_string(),
        )
        .into());
    }
    Ok(path.to_string_lossy().replace('\\', "/"))
}

/// Extracts every file under `prefix` in the archive into `target_dir`,
/// preserving the directory structure below the prefix. Returns the number of
/// files written.
pub(crate) async fn extract_archive_subdir(
    archive_path: PathBuf,
    prefix: String,
    target_dir: PathBuf,
) -> crate::Result<u32> {
    tokio::task::spawn_blocking(move || {
        extract_archive_subdir_sync(&archive_path, &prefix, &target_dir, None)
    })
    .await?
}

pub(crate) async fn extract_archive_subdir_for_instance(
    instance_id: String,
    cancellation: CancellationToken,
    archive_path: PathBuf,
    prefix: String,
    target_dir: PathBuf,
) -> crate::Result<u32> {
    run_blocking_instance_write(
        instance_id,
        cancellation,
        move |cancellation| {
            extract_archive_subdir_sync(
                &archive_path,
                &prefix,
                &target_dir,
                Some(cancellation),
            )
        },
    )
    .await
}

pub(crate) async fn run_blocking_instance_write<T, F>(
    instance_id: String,
    cancellation: CancellationToken,
    operation: F,
) -> crate::Result<T>
where
    T: Send + 'static,
    F: FnOnce(&CancellationToken) -> crate::Result<T> + Send + 'static,
{
    let state = crate::State::get().await?;
    let instance_lock =
        state.lock_instance_content_exclusive(&instance_id).await;
    tokio::task::spawn_blocking(move || {
        let _instance_lock = instance_lock;
        operation(&cancellation)
    })
    .await?
}

fn extract_archive_subdir_sync(
    archive_path: &Path,
    prefix: &str,
    target_dir: &Path,
    cancellation: Option<&CancellationToken>,
) -> crate::Result<u32> {
    let mut replacements = HashMap::<PathBuf, Option<PathBuf>>::new();
    let mut replacement_order = Vec::<PathBuf>::new();
    let result = extract_archive_subdir_entries_sync(
        archive_path,
        prefix,
        target_dir,
        cancellation,
        &mut replacements,
        &mut replacement_order,
    );
    match result {
        Ok(files_written) => {
            finalize_archive_replacements(&replacements)?;
            Ok(files_written)
        }
        Err(error) => {
            if let Err(rollback_error) =
                rollback_archive_replacements(&replacements, &replacement_order)
            {
                return Err(crate::ErrorKind::OtherError(format!(
                    "{error}; failed to restore archive extraction: {rollback_error}"
                ))
                .into());
            }
            Err(error)
        }
    }
}

fn extract_archive_subdir_entries_sync(
    archive_path: &Path,
    prefix: &str,
    target_dir: &Path,
    cancellation: Option<&CancellationToken>,
    replacements: &mut HashMap<PathBuf, Option<PathBuf>>,
    replacement_order: &mut Vec<PathBuf>,
) -> crate::Result<u32> {
    let file = std::fs::File::open(archive_path)
        .map_err(|error| io::IOError::with_path(error, archive_path))?;
    let mut archive = zip::ZipArchive::new(file).map_err(archive_error)?;
    let mut files_written = 0_u32;
    let mut total_size = 0_u64;
    for index in 0..archive.len() {
        check_cancellation(cancellation)?;
        let mut entry = archive.by_index(index).map_err(archive_error)?;
        let entry_name = decode_zip_entry_name(entry.name_raw());
        if entry.is_dir() || !entry_name.starts_with(prefix) {
            continue;
        }
        let relative = &entry_name[prefix.len()..];
        if relative.is_empty() {
            continue;
        }
        total_size = total_size.saturating_add(entry.size());
        if total_size > EXTRACTION_SIZE_LIMIT {
            return Err(crate::ErrorKind::InputError(
                "Modpack archive contents exceed the extraction limit"
                    .to_string(),
            )
            .into());
        }
        let target = target_dir.join(safe_relative_path(relative)?);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| io::IOError::with_path(error, parent))?;
        }
        write_archive_entry_atomically(
            &mut entry,
            &target,
            cancellation,
            replacements,
            replacement_order,
        )?;
        files_written = files_written.saturating_add(1);
    }
    Ok(files_written)
}

fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let mut sibling = path.as_os_str().to_os_string();
    sibling.push(suffix);
    PathBuf::from(sibling)
}

fn remove_file_if_exists(path: &Path) -> crate::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io::IOError::with_path(error, path).into()),
    }
}

fn write_archive_entry_atomically<R: std::io::Read>(
    reader: &mut R,
    target: &Path,
    cancellation: Option<&CancellationToken>,
    replacements: &mut HashMap<PathBuf, Option<PathBuf>>,
    replacement_order: &mut Vec<PathBuf>,
) -> crate::Result<u64> {
    let written = write_archive_entry_to_staging(reader, target, cancellation)?;
    materialize_staged_archive_entry(target, replacements, replacement_order)?;
    Ok(written)
}

/// Writes one archive entry completely to a sibling staging file. This is
/// safe to call from parallel extraction workers because it never mutates the
/// live target.
pub(crate) fn write_archive_entry_to_staging<R: std::io::Read>(
    reader: &mut R,
    target: &Path,
    cancellation: Option<&CancellationToken>,
) -> crate::Result<u64> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| io::IOError::with_path(error, parent))?;
    }
    let temporary = sibling_path(target, ".installing");
    remove_file_if_exists(&temporary)?;
    let write_result = (|| {
        let mut output = std::fs::File::create(&temporary)
            .map_err(|error| io::IOError::with_path(error, &temporary))?;
        let written = copy_with_cancellation(
            reader,
            &mut output,
            cancellation,
            &temporary,
        )?;
        Ok::<_, crate::Error>(written)
    })();
    let written = match write_result {
        Ok(written) => written,
        Err(error) => {
            let _ = remove_file_if_exists(&temporary);
            return Err(error);
        }
    };
    Ok(written)
}

fn materialize_staged_archive_entry(
    target: &Path,
    replacements: &mut HashMap<PathBuf, Option<PathBuf>>,
    replacement_order: &mut Vec<PathBuf>,
) -> crate::Result<()> {
    let temporary = sibling_path(target, ".installing");
    if replacements.contains_key(target) {
        // Duplicate archive entries for the same normalized path replace the
        // previous entry from this transaction, while the original pre-install
        // backup remains untouched for rollback.
        remove_file_if_exists(target)?;
        if let Err(error) = std::fs::rename(&temporary, target) {
            let _ = remove_file_if_exists(&temporary);
            return Err(io::IOError::with_path(error, target).into());
        }
        return Ok(());
    }

    let backup = sibling_path(target, ".installing.previous");
    remove_file_if_exists(&backup)?;
    let previous = if target.exists() {
        std::fs::rename(target, &backup)
            .map_err(|error| io::IOError::with_path(error, target))?;
        Some(backup)
    } else {
        None
    };
    if let Err(error) = std::fs::rename(&temporary, target) {
        if let Some(previous) = previous.as_deref() {
            let _ = std::fs::rename(previous, target);
        }
        let _ = remove_file_if_exists(&temporary);
        return Err(io::IOError::with_path(error, target).into());
    }
    replacements.insert(target.to_path_buf(), previous);
    replacement_order.push(target.to_path_buf());
    Ok(())
}

/// Publishes a set of fully-written staging files as one rollback-capable
/// batch. Targets must be unique. Cancellation before or during the short
/// rename phase restores every target already replaced by this call.
pub(crate) fn commit_staged_archive_entries(
    targets: &[PathBuf],
    cancellation: Option<&CancellationToken>,
) -> crate::Result<()> {
    let mut replacements = HashMap::<PathBuf, Option<PathBuf>>::new();
    let mut replacement_order = Vec::<PathBuf>::new();
    let result = (|| {
        for target in targets {
            check_cancellation(cancellation)?;
            materialize_staged_archive_entry(
                target,
                &mut replacements,
                &mut replacement_order,
            )?;
        }
        check_cancellation(cancellation)
    })();
    if let Err(error) = result {
        if let Err(rollback_error) =
            rollback_archive_replacements(&replacements, &replacement_order)
        {
            return Err(crate::ErrorKind::OtherError(format!(
                "{error}; failed to restore staged archive entries: {rollback_error}"
            ))
            .into());
        }
        let _ = discard_staged_archive_entries(targets);
        return Err(error);
    }
    // Backup deletion happens after the transactional commit point. A cleanup
    // failure must not invoke rollback because earlier backups may already be
    // gone; the caller's install-level recovery can handle the reported error.
    finalize_archive_replacements(&replacements)
}

pub(crate) fn discard_staged_archive_entries(
    targets: &[PathBuf],
) -> crate::Result<()> {
    for target in targets {
        remove_file_if_exists(&sibling_path(target, ".installing"))?;
    }
    Ok(())
}

fn finalize_archive_replacements(
    replacements: &HashMap<PathBuf, Option<PathBuf>>,
) -> crate::Result<()> {
    for previous in replacements.values().flatten() {
        remove_file_if_exists(previous)?;
    }
    Ok(())
}

fn rollback_archive_replacements(
    replacements: &HashMap<PathBuf, Option<PathBuf>>,
    replacement_order: &[PathBuf],
) -> crate::Result<()> {
    for target in replacement_order.iter().rev() {
        remove_file_if_exists(target)?;
        if let Some(previous) =
            replacements.get(target).and_then(Option::as_ref)
            && previous.exists()
        {
            std::fs::rename(previous, target)
                .map_err(|error| io::IOError::with_path(error, target))?;
        }
        let temporary = sibling_path(target, ".installing");
        remove_file_if_exists(&temporary)?;
    }
    Ok(())
}

pub(crate) fn check_cancellation(
    cancellation: Option<&CancellationToken>,
) -> crate::Result<()> {
    if cancellation.is_some_and(CancellationToken::is_cancelled) {
        return Err(crate::ErrorKind::OtherError(
            "Install was canceled".to_string(),
        )
        .into());
    }
    Ok(())
}

pub(crate) fn copy_with_cancellation<R, W>(
    reader: &mut R,
    writer: &mut W,
    cancellation: Option<&CancellationToken>,
    target: &Path,
) -> crate::Result<u64>
where
    R: std::io::Read,
    W: std::io::Write,
{
    let mut buffer = [0_u8; 64 * 1024];
    let mut written = 0_u64;
    loop {
        check_cancellation(cancellation)?;
        let count = reader
            .read(&mut buffer)
            .map_err(|error| io::IOError::with_path(error, target))?;
        if count == 0 {
            return Ok(written);
        }
        check_cancellation(cancellation)?;
        writer
            .write_all(&buffer[..count])
            .map_err(|error| io::IOError::with_path(error, target))?;
        written = written.saturating_add(count as u64);
    }
}

/// Extracts a single archive entry to the given target file path.
pub(crate) async fn extract_archive_entry_to_file(
    archive_path: PathBuf,
    entry_name: String,
    target: PathBuf,
) -> crate::Result<()> {
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&archive_path)
            .map_err(|error| io::IOError::with_path(error, &archive_path))?;
        let mut archive = zip::ZipArchive::new(file).map_err(archive_error)?;
        let index = (0..archive.len())
            .find(|&index| {
                archive
                    .by_index_raw(index)
                    .map(|entry| {
                        decode_zip_entry_name(entry.name_raw()) == entry_name
                    })
                    .unwrap_or(false)
            })
            .ok_or_else(|| {
                crate::ErrorKind::InputError(format!(
                    "Modpack archive is missing {entry_name}"
                ))
            })?;
        let mut entry = archive.by_index(index).map_err(archive_error)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| io::IOError::with_path(error, parent))?;
        }
        let mut output = std::fs::File::create(&target)
            .map_err(|error| io::IOError::with_path(error, &target))?;
        std::io::copy(&mut entry, &mut output)
            .map_err(|error| io::IOError::with_path(error, &target))?;
        Ok(())
    })
    .await?
}

/// Reads a single archive entry into a string, tolerating GB18030-encoded
/// file contents produced by Chinese packaging tools.
pub(crate) async fn read_archive_entry_to_string(
    archive_path: PathBuf,
    entry_name: String,
) -> crate::Result<String> {
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&archive_path)
            .map_err(|error| io::IOError::with_path(error, &archive_path))?;
        let mut archive = zip::ZipArchive::new(file).map_err(archive_error)?;
        let index = super::detect::find_entry_index(&mut archive, &entry_name)?
            .ok_or_else(|| {
                crate::ErrorKind::InputError(format!(
                    "Modpack archive is missing {entry_name}"
                ))
            })?;
        let mut entry = archive.by_index(index).map_err(archive_error)?;
        let mut contents = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut contents)?;
        Ok(match String::from_utf8(contents) {
            Ok(value) => value,
            Err(error) => {
                let (decoded, _, _) =
                    encoding_rs::GB18030.decode(error.as_bytes());
                decoded.into_owned()
            }
        })
    })
    .await?
}

/// Allocates a unique scratch directory for extracting nested pack content.
pub(crate) async fn create_import_scratch_dir(
    state: &crate::State,
) -> crate::Result<PathBuf> {
    let dir = state
        .directories
        .caches_dir()
        .join("modpack-import")
        .join(uuid::Uuid::new_v4().to_string());
    io::create_dir_all(&dir).await?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::io::Write;

    struct CancelOnRead {
        cancellation: CancellationToken,
        read: bool,
    }

    impl std::io::Read for CancelOnRead {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.read {
                return Ok(0);
            }
            self.read = true;
            self.cancellation.cancel();
            let count = buffer.len().min(1024);
            buffer[..count].fill(1);
            Ok(count)
        }
    }

    #[test]
    fn blocking_copy_stops_before_writing_after_cancellation() {
        let cancellation = CancellationToken::new();
        let mut reader = CancelOnRead {
            cancellation: cancellation.clone(),
            read: false,
        };
        let mut output = Vec::new();
        let error = copy_with_cancellation(
            &mut reader,
            &mut output,
            Some(&cancellation),
            Path::new("override.bin"),
        )
        .unwrap_err();

        assert!(error.to_string().contains("Install was canceled"));
        assert!(output.is_empty());
    }

    #[test]
    fn canceled_atomic_entry_keeps_existing_target() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("config/example.bin");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"previous").unwrap();
        let cancellation = CancellationToken::new();
        let mut reader = CancelOnRead {
            cancellation: cancellation.clone(),
            read: false,
        };
        let mut replacements = HashMap::new();
        let mut order = Vec::new();

        let error = write_archive_entry_atomically(
            &mut reader,
            &target,
            Some(&cancellation),
            &mut replacements,
            &mut order,
        )
        .unwrap_err();

        assert!(error.to_string().contains("Install was canceled"));
        assert_eq!(std::fs::read(&target).unwrap(), b"previous");
        assert!(!sibling_path(&target, ".installing").exists());
        assert!(replacements.is_empty());
    }

    #[test]
    fn archive_batch_rollback_restores_all_original_files() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("config/first.txt");
        let second = root.path().join("config/second.txt");
        std::fs::create_dir_all(first.parent().unwrap()).unwrap();
        std::fs::write(&first, b"old-first").unwrap();
        std::fs::write(&second, b"old-second").unwrap();
        let mut replacements = HashMap::new();
        let mut order = Vec::new();

        write_archive_entry_atomically(
            &mut Cursor::new(b"new-first"),
            &first,
            None,
            &mut replacements,
            &mut order,
        )
        .unwrap();
        write_archive_entry_atomically(
            &mut Cursor::new(b"new-second"),
            &second,
            None,
            &mut replacements,
            &mut order,
        )
        .unwrap();
        rollback_archive_replacements(&replacements, &order).unwrap();

        assert_eq!(std::fs::read(&first).unwrap(), b"old-first");
        assert_eq!(std::fs::read(&second).unwrap(), b"old-second");
        assert!(!sibling_path(&first, ".installing.previous").exists());
        assert!(!sibling_path(&second, ".installing.previous").exists());
    }

    #[test]
    fn duplicate_archive_entries_preserve_preinstall_backup() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("config/example.txt");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"original").unwrap();
        let mut replacements = HashMap::new();
        let mut order = Vec::new();

        write_archive_entry_atomically(
            &mut Cursor::new(b"first-entry"),
            &target,
            None,
            &mut replacements,
            &mut order,
        )
        .unwrap();
        write_archive_entry_atomically(
            &mut Cursor::new(b"second-entry"),
            &target,
            None,
            &mut replacements,
            &mut order,
        )
        .unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"second-entry");

        rollback_archive_replacements(&replacements, &order).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
    }

    #[test]
    fn successful_archive_batch_removes_previous_backups() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("config/example.txt");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"original").unwrap();
        let mut replacements = HashMap::new();
        let mut order = Vec::new();

        write_archive_entry_atomically(
            &mut Cursor::new(b"replacement"),
            &target,
            None,
            &mut replacements,
            &mut order,
        )
        .unwrap();
        finalize_archive_replacements(&replacements).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"replacement");
        assert!(!sibling_path(&target, ".installing.previous").exists());
    }

    #[test]
    fn corrupt_later_zip_entry_rolls_back_earlier_replacements() {
        let root = tempfile::tempdir().unwrap();
        let archive_path = root.path().join("pack.zip");
        let target_dir = root.path().join("instance");
        let first = target_dir.join("config/first.txt");
        let second = target_dir.join("config/second.txt");
        std::fs::create_dir_all(first.parent().unwrap()).unwrap();
        std::fs::write(&first, b"old-first").unwrap();
        std::fs::write(&second, b"old-second").unwrap();

        let second_payload = b"unique-second-entry-payload";
        {
            let file = std::fs::File::create(&archive_path).unwrap();
            let mut archive = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            archive
                .start_file("overrides/config/first.txt", options)
                .unwrap();
            archive.write_all(b"new-first").unwrap();
            archive
                .start_file("overrides/config/second.txt", options)
                .unwrap();
            archive.write_all(second_payload).unwrap();
            archive.finish().unwrap();
        }
        let mut bytes = std::fs::read(&archive_path).unwrap();
        let payload_offset = bytes
            .windows(second_payload.len())
            .position(|window| window == second_payload)
            .expect("stored ZIP payload should be directly addressable");
        bytes[payload_offset] ^= 0xff;
        std::fs::write(&archive_path, bytes).unwrap();

        let error = extract_archive_subdir_sync(
            &archive_path,
            "overrides/",
            &target_dir,
            None,
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("CRC")
                || error.to_string().contains("checksum")
        );
        assert_eq!(std::fs::read(&first).unwrap(), b"old-first");
        assert_eq!(std::fs::read(&second).unwrap(), b"old-second");
        assert!(!sibling_path(&first, ".installing").exists());
        assert!(!sibling_path(&first, ".installing.previous").exists());
        assert!(!sibling_path(&second, ".installing").exists());
        assert!(!sibling_path(&second, ".installing.previous").exists());
    }
}
