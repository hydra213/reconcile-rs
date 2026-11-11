//! Finding the files a location names and reading their bytes.
//!
//! A location is either a local path or an object storage URL. This is the
//! only module that knows the difference, and the only one that talks to
//! object storage.
//!
//! | Location | Meaning |
//! |---|---|
//! | `data/trades.parquet` | one local file |
//! | `data/trades/` (a folder) | every data file directly inside the folder |
//! | `s3://bucket/trades.parquet` | one object |
//! | `s3://bucket/trades/` (ends with `/`) | every data file directly under the prefix |
//!
//! "Data files" are files ending in `.parquet` or `.csv`. Marker files that
//! engines write next to the data, such as `_SUCCESS`, are skipped.
//! Sub-folders are not searched.

use std::path::Path;

use bytes::Bytes;
use object_store::{ObjectStore, ObjectStoreExt};
use url::Url;

use crate::error::Error;

/// One input file, read into memory.
#[derive(Debug, Clone)]
pub struct FetchedFile {
    /// Where the file was read from, as a path or URL.
    pub location: String,
    pub bytes: Bytes,
}

/// Reads every file the locations name, in the order they were given.
/// Files of a folder come in name order. Fails if there is no file at all.
pub fn fetch_all(locations: &[String]) -> Result<Vec<FetchedFile>, Error> {
    let mut files = Vec::new();
    for location in locations {
        if is_url(location) {
            files.extend(fetch_remote(location)?);
        } else {
            files.extend(fetch_local(location)?);
        }
    }

    if files.is_empty() {
        return Err(Error::NoInputFiles);
    }
    Ok(files)
}

/// `s3://bucket/key` is a URL; `C:\data\file.parquet` and `data/file` are
/// local paths.
fn is_url(location: &str) -> bool {
    location.contains("://")
}

fn is_data_file(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".parquet") || name.ends_with(".csv")
}

// ---------------------------------------------------------------------------
// Local files
// ---------------------------------------------------------------------------

fn fetch_local(location: &str) -> Result<Vec<FetchedFile>, Error> {
    let path = Path::new(location);
    if !path.is_dir() {
        return Ok(vec![read_local_file(location)?]);
    }

    let read_error = |source| Error::ReadFile {
        location: location.to_string(),
        source,
    };
    let mut names = Vec::new();
    for entry in std::fs::read_dir(path).map_err(read_error)? {
        let entry = entry.map_err(read_error)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().is_file() && is_data_file(&name) {
            names.push(name);
        }
    }
    names.sort();

    if names.is_empty() {
        return Err(Error::NoDataFilesInFolder {
            location: location.to_string(),
        });
    }
    names
        .iter()
        .map(|name| read_local_file(&path.join(name).to_string_lossy()))
        .collect()
}

fn read_local_file(location: &str) -> Result<FetchedFile, Error> {
    let bytes = std::fs::read(location).map_err(|source| Error::ReadFile {
        location: location.to_string(),
        source,
    })?;
    Ok(FetchedFile {
        location: location.to_string(),
        bytes: Bytes::from(bytes),
    })
}

// ---------------------------------------------------------------------------
// Object storage
// ---------------------------------------------------------------------------

/// Reads one object, or every data file under a prefix that ends with `/`.
///
/// Credentials and settings come from the environment variables each cloud
/// uses, for example `AWS_ACCESS_KEY_ID`, `AWS_REGION`,
/// `AZURE_STORAGE_ACCOUNT_NAME` or `GOOGLE_SERVICE_ACCOUNT`. On a cloud
/// machine with an attached role or identity, nothing needs to be set.
fn fetch_remote(location: &str) -> Result<Vec<FetchedFile>, Error> {
    let url = Url::parse(location).map_err(|source| Error::InvalidUrl {
        location: location.to_string(),
        source,
    })?;
    let storage_error = |source| Error::ReadObjectStorage {
        location: location.to_string(),
        source,
    };

    // `store` is the bucket or container, `path` is the place inside it.
    let (store, path) =
        object_store::parse_url_opts(&url, std::env::vars()).map_err(storage_error)?;

    // The object storage library is asynchronous. This program does one
    // thing at a time, so a small runtime that runs each request to its end
    // is all it needs.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|source| Error::ReadFile {
            location: location.to_string(),
            source,
        })?;

    if !location.ends_with('/') {
        let bytes = runtime
            .block_on(read_object(&*store, &path))
            .map_err(storage_error)?;
        return Ok(vec![FetchedFile {
            location: location.to_string(),
            bytes,
        }]);
    }

    let listing = runtime
        .block_on(store.list_with_delimiter(Some(&path)))
        .map_err(storage_error)?;
    let mut objects: Vec<_> = listing
        .objects
        .into_iter()
        .filter(|object| is_data_file(object.location.as_ref()))
        .collect();
    objects.sort_by(|a, b| a.location.cmp(&b.location));

    if objects.is_empty() {
        return Err(Error::NoDataFilesInFolder {
            location: location.to_string(),
        });
    }

    let mut files = Vec::new();
    for object in objects {
        let bytes = runtime
            .block_on(read_object(&*store, &object.location))
            .map_err(storage_error)?;
        // The folder URL ends with `/`, so adding the file name gives the
        // URL of the file.
        let file_name = object.location.filename().unwrap_or_default();
        files.push(FetchedFile {
            location: format!("{location}{file_name}"),
            bytes,
        });
    }
    Ok(files)
}

async fn read_object(
    store: &dyn ObjectStore,
    path: &object_store::path::Path,
) -> Result<Bytes, object_store::Error> {
    store.get(path).await?.bytes().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_local_paths_are_told_apart() {
        assert!(is_url("s3://bucket/trades.parquet"));
        assert!(is_url(
            "abfss://container@account.dfs.core.windows.net/trades/"
        ));
        assert!(!is_url("data/trades.parquet"));
        assert!(!is_url(r"C:\data\trades.parquet"));
    }

    #[test]
    fn only_parquet_and_csv_files_count_as_data() {
        assert!(is_data_file("part-00000.snappy.parquet"));
        assert!(is_data_file("TRADES.CSV"));
        assert!(!is_data_file("_SUCCESS"));
        assert!(!is_data_file(".part-00000.parquet.crc"));
    }

    #[test]
    fn no_locations_is_an_error() {
        assert!(matches!(fetch_all(&[]), Err(Error::NoInputFiles)));
    }
}
