use crate::{Registration, NEXT_MAP_INDEX, REGISTER};
use std::{io, path::Path};
use tempfile::NamedTempFile;

impl Registration {
    /// Create a registered tempfile at the given `path`, where `path` includes the desired filename.
    ///
    /// **Note** that intermediate directories will _not_ be created.
    pub fn at_path(path: impl AsRef<Path>) -> io::Result<Registration> {
        let path = path.as_ref();
        let id = NEXT_MAP_INDEX.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        expect_none(REGISTER.insert(
            id,
            Some({
                let mut builder = tempfile::Builder::new();
                let dot_ext_storage;
                match path.file_stem() {
                    Some(stem) => builder.prefix(stem),
                    None => builder.prefix(""),
                };
                if let Some(ext) = path.extension() {
                    dot_ext_storage = format!(".{}", ext.to_string_lossy());
                    builder.suffix(&dot_ext_storage);
                }
                builder
                    .rand_bytes(0)
                    .tempfile_in(path.parent().expect("parent directory is present"))?
                    .into()
            }),
        ));
        Ok(Registration { id })
    }

    /// Create a registered tempfile within `containing_directory` with a name that won't clash.
    /// **Note** that intermediate directories will _not_ be created.
    pub fn new(containing_directory: impl AsRef<Path>) -> io::Result<Registration> {
        let id = NEXT_MAP_INDEX.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        expect_none(REGISTER.insert(id, Some(NamedTempFile::new_in(containing_directory)?.into())));
        Ok(Registration { id })
    }

    /// Take ownership of the temporary file.
    ///
    pub fn take(self) -> Option<NamedTempFile> {
        let res = REGISTER.remove(&self.id);
        std::mem::forget(self);
        res.and_then(|(_k, v)| v.map(|v| v.inner))
    }
}

fn expect_none<T>(v: Option<T>) {
    assert!(
        v.is_none(),
        "there should never be conflicts or old values as ids are never reused."
    );
}

impl Drop for Registration {
    fn drop(&mut self) {
        REGISTER.remove(&self.id);
    }
}