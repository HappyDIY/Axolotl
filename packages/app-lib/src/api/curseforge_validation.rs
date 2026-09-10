use crate::{ErrorKind, Result};
use std::path::Path;

use std::path::Component;

pub(super) fn validate_file_name(file_name: &str) -> Result<()> {
    let path = Path::new(file_name);
    if file_name.is_empty()
        || path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        return Err(ErrorKind::InputError(
            "CurseForge returned an invalid file name".to_string(),
        )
        .into());
    }
    Ok(())
}

pub(super) fn validate_world_archive_name(file_name: &str) -> Result<()> {
    validate_file_name(file_name)?;
    if Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        return Ok(());
    }
    Err(ErrorKind::InputError(
        "CurseForge world downloads must be ZIP archives".to_string(),
    )
    .into())
}
