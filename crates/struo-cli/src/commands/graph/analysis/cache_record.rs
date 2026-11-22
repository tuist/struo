use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::Result;
use serde::Serialize;
use tempfile::NamedTempFile;

pub(super) fn stage_json(directory: &Path, record: &impl Serialize) -> Result<NamedTempFile> {
    std::fs::create_dir_all(directory)?;
    let mut temporary = NamedTempFile::new_in(directory)?;
    let mut writer = BufWriter::new(temporary.as_file_mut());
    serde_json::to_writer(&mut writer, record)?;
    writer.flush()?;
    drop(writer);
    Ok(temporary)
}
