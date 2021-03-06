/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::errors::BuildProjectError;
use log::info;
use serde::{Serialize, Serializer};
use std::fs::{create_dir_all, File};
use std::io;
use std::io::prelude::*;
use std::{path::PathBuf, sync::Mutex};

type BuildProjectResult = Result<(), BuildProjectError>;

pub trait ArtifactWriter {
    fn write_if_changed(&self, path: PathBuf, content: Vec<u8>) -> BuildProjectResult;
    fn remove(&self, path: PathBuf) -> BuildProjectResult;
    fn finalize(&self) -> crate::errors::Result<()>;
}

pub struct ArtifactFileWriter;

impl ArtifactWriter for ArtifactFileWriter {
    fn write_if_changed(&self, path: PathBuf, content: Vec<u8>) -> BuildProjectResult {
        write_file(&path, &content).map_err(|error| BuildProjectError::WriteFileError {
            file: path,
            source: error,
        })
    }

    fn remove(&self, path: PathBuf) -> BuildProjectResult {
        std::fs::remove_file(&path).unwrap_or_else(|_| {
            info!("tried to delete already deleted file: {:?}", path);
        });
        Ok(())
    }

    fn finalize(&self) -> crate::errors::Result<()> {
        Ok(())
    }
}

#[derive(Serialize)]
struct CodegenRecords {
    pub removed: Vec<ArtifactDeletionRecord>,
    pub changed: Vec<ArtifactUpdateRecord>,
}

#[derive(Serialize)]
struct ArtifactDeletionRecord {
    pub path: PathBuf,
}

#[derive(Serialize)]
struct ArtifactUpdateRecord {
    pub path: PathBuf,
    #[serde(serialize_with = "from_utf8")]
    pub data: Vec<u8>,
}

fn from_utf8<S>(slice: &Vec<u8>, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    s.serialize_str(std::str::from_utf8(slice).unwrap())
}

pub struct ArtifactDifferenceWriter {
    codegen_records: Mutex<CodegenRecords>,
    codegen_filepath: PathBuf,
    verify_changes_against_filesystem: bool,
}

impl ArtifactDifferenceWriter {
    pub fn new(
        codegen_filepath: PathBuf,
        verify_changes_against_filesystem: bool,
    ) -> ArtifactDifferenceWriter {
        ArtifactDifferenceWriter {
            codegen_filepath,
            codegen_records: Mutex::new(CodegenRecords {
                changed: Vec::new(),
                removed: Vec::new(),
            }),
            verify_changes_against_filesystem,
        }
    }
}

impl ArtifactWriter for ArtifactDifferenceWriter {
    fn write_if_changed(&self, path: PathBuf, content: Vec<u8>) -> BuildProjectResult {
        let should_include_artifact_in_codegen = !self.verify_changes_against_filesystem
            || !content_is_same(&path, &content).map_err(|error| {
                BuildProjectError::WriteFileError {
                    file: path.clone(),
                    source: error,
                }
            })?;
        if should_include_artifact_in_codegen {
            self.codegen_records
                .lock()
                .unwrap()
                .changed
                .push(ArtifactUpdateRecord {
                    path,
                    data: content,
                });
        }
        Ok(())
    }

    fn remove(&self, path: PathBuf) -> BuildProjectResult {
        if path.exists() {
            self.codegen_records
                .lock()
                .unwrap()
                .removed
                .push(ArtifactDeletionRecord { path });
        }
        Ok(())
    }

    fn finalize(&self) -> crate::errors::Result<()> {
        (|| {
            let mut file = File::create(&self.codegen_filepath)?;
            file.write_all(&serde_json::to_string(&self.codegen_records)?.as_bytes())
        })()
        .map_err(|error| crate::errors::Error::WriteFileError {
            file: self.codegen_filepath.clone(),
            source: error,
        })
    }
}

fn ensure_file_directory_exists(file_path: &PathBuf) -> io::Result<()> {
    if let Some(file_directory) = file_path.parent() {
        if !file_directory.exists() {
            create_dir_all(file_directory)?;
        }
    }

    Ok(())
}

fn write_file(path: &PathBuf, content: &[u8]) -> io::Result<()> {
    if path.exists() {
        let existing_content = std::fs::read(path)?;
        if existing_content == content {
            return Ok(());
        }
    } else {
        ensure_file_directory_exists(path)?;
    }

    let mut file = File::create(path)?;
    file.write_all(&content)?;
    Ok(())
}

fn content_is_same(path: &PathBuf, content: &Vec<u8>) -> io::Result<bool> {
    if path.exists() {
        let existing_content = std::fs::read(path)?;
        Ok(&existing_content == content)
    } else {
        Ok(false)
    }
}
