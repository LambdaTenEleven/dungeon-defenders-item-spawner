//! Read-only parser for the installed UE3 v678/v679, licensee-2 package format.
//! The format gate is independent of the executable gate used for native calls.

#[path = "lzo.rs"]
mod lzo;

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::{Value, json};

use crate::{ClassDefinition, ExportRecord};

const SIGNATURE: u32 = 0x9e2a83c1;
const MAX_FILE: usize = 512 * 1024 * 1024;
const MAX_LOGICAL: usize = 1024 * 1024 * 1024;
const MAX_TABLE: usize = 128 * 1024 * 1024;
const MAX_CHUNK: usize = 32 * 1024 * 1024;
const MAX_EXPORT_DATA: usize = 16 * 1024 * 1024;
const MAX_RECORDS: usize = 1_000_000;

#[derive(Clone)]
struct Object {
    outer: i32,
    name: String,
    class: i32,
    parent: i32,
    archetype: i32,
    flags: u64,
    export_flags: u32,
    offset: usize,
    size: usize,
}

#[derive(Clone, Copy)]
struct Chunk {
    logical: usize,
    size: usize,
    physical: usize,
    compressed: usize,
}

enum Input {
    Memory(Vec<u8>),
    File { file: File, length: usize },
}

impl Input {
    fn len(&self) -> usize {
        match self {
            Self::Memory(bytes) => bytes.len(),
            Self::File { length, .. } => *length,
        }
    }

    fn read(&mut self, offset: usize, count: usize) -> Result<Vec<u8>, String> {
        let end = offset
            .checked_add(count)
            .ok_or("Physical package range overflow")?;
        if count > MAX_TABLE || end > self.len() {
            return Err("Physical package read outside bounds".into());
        }
        match self {
            Self::Memory(bytes) => Ok(bytes[offset..end].to_vec()),
            Self::File { file, .. } => {
                file.seek(SeekFrom::Start(offset as u64))
                    .map_err(|error| error.to_string())?;
                let mut bytes = vec![0; count];
                file.read_exact(&mut bytes)
                    .map_err(|error| format!("Truncated or unreadable package range: {error}"))?;
                Ok(bytes)
            }
        }
    }
}

pub struct Package {
    raw: Input,
    stem: String,
    names: Vec<String>,
    imports: Vec<Object>,
    exports: Vec<Object>,
    paths: HashMap<i32, String>,
    chunks: Vec<Chunk>,
    logical_size: usize,
    header_size: usize,
    chunk_cache: Option<(usize, Vec<u8>)>,
}

impl Package {
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|error| error.to_string())?;
        let size = file.metadata().map_err(|error| error.to_string())?.len();
        if size > MAX_FILE as u64 {
            return Err("Package exceeds 512 MiB parser limit".into());
        }
        let mut raw = Vec::with_capacity(size as usize);
        file.take(MAX_FILE as u64 + 1)
            .read_to_end(&mut raw)
            .map_err(|error| error.to_string())?;
        if raw.len() > MAX_FILE {
            return Err("Package grew beyond parser limit".into());
        }
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or("Package filename is not Unicode")?;
        Self::parse(raw, stem)
    }

    /// Stream physical ranges instead of retaining the entire compressed file.
    /// This is the reader to use inside the 32-bit adapter. Only the metadata
    /// tables and one decompressed chunk are retained after opening.
    pub fn open_streamed(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|error| error.to_string())?;
        let length = file.metadata().map_err(|error| error.to_string())?.len();
        if length > MAX_FILE as u64 {
            return Err("Package exceeds 512 MiB parser limit".into());
        }
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or("Package filename is not Unicode")?;
        Self::from_input(
            Input::File {
                file,
                length: length as usize,
            },
            stem,
        )
    }

    /// In-memory source bytes are available only with `open`, never with
    /// `open_streamed`; streamed callers must hash the source separately.
    pub fn bytes(&self) -> Option<&[u8]> {
        match &self.raw {
            Input::Memory(bytes) => Some(bytes),
            Input::File { .. } => None,
        }
    }

    fn parse(raw: Vec<u8>, stem: &str) -> Result<Self, String> {
        Self::from_input(Input::Memory(raw), stem)
    }

    fn from_input(mut input: Input, stem: &str) -> Result<Self, String> {
        let raw = input.read(0, input.len().min(89))?;
        if u32_at(&raw, 0)? != SIGNATURE {
            return Err("Unsupported package signature".into());
        }
        let version = u32_at(&raw, 4)?;
        if !matches!(version & 0xffff, 678 | 679) || version >> 16 != 2 {
            return Err(format!(
                "Unsupported package version {} / licensee {}",
                version & 0xffff,
                version >> 16
            ));
        }
        if raw.get(12..21) != Some(&b"\x05\0\0\0None\0"[..]) {
            return Err("Unsupported package group layout".into());
        }
        let name_count = bounded_count(&raw, 25)?;
        let name_offset = u32_at(&raw, 29)? as usize;
        let export_count = bounded_count(&raw, 33)?;
        let export_offset = u32_at(&raw, 37)? as usize;
        let import_count = bounded_count(&raw, 41)?;
        let import_offset = u32_at(&raw, 45)? as usize;
        let depends = u32_at(&raw, 49)? as usize;
        if depends > MAX_TABLE
            || !(name_offset <= import_offset
                && import_offset <= export_offset
                && export_offset <= depends)
        {
            return Err("Invalid or oversized package tables".into());
        }
        let generations = u32_at(&raw, 85)? as usize;
        if generations > 1024 {
            return Err("Too many package generations".into());
        }
        let compression_offset = 97 + generations * 12;
        let compression_header = input.read(compression_offset, 8)?;
        let compression = u32_at(&compression_header, 0)?;
        let chunk_count = u32_at(&compression_header, 4)? as usize;
        if chunk_count > 65536 || (chunk_count != 0 && compression != 2) {
            return Err("Unsupported package compression".into());
        }
        let chunk_table = input.read(compression_offset + 8, chunk_count * 16)?;
        let mut chunks = Vec::with_capacity(chunk_count);
        let mut last_end = name_offset;
        let mut physical_end = compression_offset + 8 + chunk_count * 16;
        for index in 0..chunk_count {
            let offset = index * 16;
            let chunk = Chunk {
                logical: u32_at(&chunk_table, offset)? as usize,
                size: u32_at(&chunk_table, offset + 4)? as usize,
                physical: u32_at(&chunk_table, offset + 8)? as usize,
                compressed: u32_at(&chunk_table, offset + 12)? as usize,
            };
            if chunk.size == 0
                || chunk.size > MAX_CHUNK
                || chunk.compressed > MAX_CHUNK * 2
                || chunk.logical != last_end
                || chunk
                    .logical
                    .checked_add(chunk.size)
                    .is_none_or(|end| end > MAX_LOGICAL)
                || chunk.physical < physical_end
                || chunk
                    .physical
                    .checked_add(chunk.compressed)
                    .is_none_or(|end| end > input.len())
            {
                return Err("Invalid compressed chunk range".into());
            }
            last_end = chunk.logical + chunk.size;
            physical_end = chunk.physical + chunk.compressed;
            chunks.push(chunk);
        }
        let logical_size = if chunks.is_empty() {
            input.len()
        } else {
            last_end
        };
        let mut package = Self {
            raw: input,
            stem: stem.into(),
            names: Vec::with_capacity(name_count),
            imports: Vec::with_capacity(import_count),
            exports: Vec::with_capacity(export_count),
            paths: HashMap::new(),
            chunks,
            logical_size,
            header_size: name_offset,
            chunk_cache: None,
        };
        let table = package.read(0, depends, MAX_TABLE)?;
        let mut position = name_offset;
        for _ in 0..name_count {
            let (name, consumed) = string_at(&table, position, 2048)?;
            package.names.push(name);
            position = position
                .checked_add(consumed + 8)
                .ok_or("Name table overflow")?;
        }
        if position != import_offset {
            return Err("Name table end does not match import table".into());
        }
        for _ in 0..import_count {
            package.imports.push(Object {
                outer: i32_at(&table, position + 16)?,
                name: package.fname(&table, position + 20)?,
                class: 0,
                parent: 0,
                archetype: 0,
                flags: 0,
                export_flags: 0,
                offset: 0,
                size: 0,
            });
            position += 28;
        }
        if position != export_offset {
            return Err("Import table end does not match export table".into());
        }
        for _ in 0..export_count {
            let offset = u32_at(&table, position + 36)? as usize;
            let size = u32_at(&table, position + 32)? as usize;
            let generations = u32_at(&table, position + 44)? as usize;
            if generations >= 100
                || offset
                    .checked_add(size)
                    .is_none_or(|end| end > logical_size)
            {
                return Err("Invalid export data range".into());
            }
            package.exports.push(Object {
                class: i32_at(&table, position)?,
                parent: i32_at(&table, position + 4)?,
                outer: i32_at(&table, position + 8)?,
                name: package.fname(&table, position + 12)?,
                archetype: i32_at(&table, position + 20)?,
                flags: u64::from(u32_at(&table, position + 24)?)
                    | (u64::from(u32_at(&table, position + 28)?) << 32),
                export_flags: u32_at(&table, position + 40)?,
                offset,
                size,
            });
            position += 68 + generations * 4;
        }
        if position != depends {
            return Err("Export table end does not match dependency table".into());
        }
        Ok(package)
    }

    fn resolve(&mut self, index: i32, depth: usize) -> Result<Option<String>, String> {
        if index == 0 {
            return Ok(None);
        }
        if let Some(path) = self.paths.get(&index) {
            return Ok(Some(path.clone()));
        }
        if depth >= 60 {
            return Err("Cyclic or excessively deep object outer chain".into());
        }
        let object = if index > 0 {
            self.exports.get(index as usize - 1)
        } else {
            self.imports.get(index.unsigned_abs() as usize - 1)
        }
        .ok_or("Object index outside package tables")?;
        let name = object.name.clone();
        let outer = object.outer;
        // Ordinary root exports belong to this file's package. Forced exports
        // retain their logical package root when cooked into another file. Imports
        // name external roots and must never acquire the source filename prefix.
        let local_root = index > 0 && object.export_flags & 1 == 0;
        let result = match self.resolve(outer, depth + 1)? {
            Some(parent) => format!("{parent}.{name}"),
            None if local_root => format!("{}.{}", self.stem, name),
            None => name,
        };
        if result.len() > 8192 {
            return Err("Object path exceeds length limit".into());
        }
        self.paths.insert(index, result.clone());
        Ok(Some(result))
    }

    pub fn classes(&mut self) -> Result<Vec<ClassDefinition>, String> {
        let mut classes = Vec::new();
        for index in 0..self.exports.len() {
            if self.exports[index].class != 0 {
                continue;
            }
            let path = self
                .resolve(index as i32 + 1, 0)?
                .ok_or("Class path missing")?;
            let parent = self.resolve(self.exports[index].parent, 0)?;
            classes.push(ClassDefinition { path, parent });
        }
        Ok(classes)
    }

    pub fn candidates(
        &mut self,
        graph: &BTreeMap<String, Option<String>>,
    ) -> Result<Vec<ExportRecord>, String> {
        let mut records = Vec::new();
        for index in 0..self.exports.len() {
            let object = self.exports[index].clone();
            if object.class == 0 {
                continue;
            }
            let class = self
                .resolve(object.class, 0)?
                .ok_or("Export class missing")?;
            let class_name = class;
            if !crate::rules::is_candidate(&class_name, graph) {
                continue;
            }
            let path = self
                .resolve(index as i32 + 1, 0)?
                .ok_or("Export path missing")?;
            let archetype = self.resolve(object.archetype, 0)?;
            let (properties, property_error) = match self.properties(&object) {
                Ok(properties) => (properties, None),
                Err(error) => (BTreeMap::new(), Some(error)),
            };
            records.push(ExportRecord {
                path,
                class_name,
                archetype,
                is_archetype: object.flags & 0x400 != 0,
                is_class_default: object.name.starts_with("Default__"),
                properties,
                property_error,
            });
        }
        Ok(records)
    }

    /// Read the selected export directly from its source package, independently
    /// of cached classification. UE object names compare without ASCII case;
    /// prefixes, suffixes, and alternate outer paths are never accepted.
    pub fn inspect_export(&mut self, requested_path: &str) -> Result<ExportRecord, String> {
        self.inspect_record(requested_path, true)
    }

    /// Asset leaves need table identity only, not their opaque serialized payload.
    pub fn inspect_identity(&mut self, requested_path: &str) -> Result<ExportRecord, String> {
        self.inspect_record(requested_path, false)
    }

    fn inspect_record(
        &mut self,
        requested_path: &str,
        properties: bool,
    ) -> Result<ExportRecord, String> {
        if requested_path.is_empty() || requested_path.len() > 8192 {
            return Err("Selected export path is empty or oversized".into());
        }
        let mut selected = None;
        let leaf = requested_path
            .rsplit('.')
            .next()
            .ok_or("Selected export leaf is missing")?;
        for index in 0..self.exports.len() {
            if self.exports[index].class == 0
                || !self.exports[index].name.eq_ignore_ascii_case(leaf)
            {
                continue;
            }
            let path = self
                .resolve(index as i32 + 1, 0)?
                .ok_or("Export path missing")?;
            if path.eq_ignore_ascii_case(requested_path)
                && selected.replace((index, path)).is_some()
            {
                return Err("Selected path has ambiguous exports in its source package".into());
            }
        }
        let (index, path) =
            selected.ok_or("Selected export was not found in its source package")?;
        let object = self.exports[index].clone();
        let class = self
            .resolve(object.class, 0)?
            .ok_or("Selected export class is missing")?;
        let class_name = class;
        let archetype = self.resolve(object.archetype, 0)?;
        let (properties, property_error) = match if properties {
            self.properties(&object)
        } else {
            Ok(BTreeMap::new())
        } {
            Ok(properties) => (properties, None),
            Err(error) => (BTreeMap::new(), Some(error)),
        };
        Ok(ExportRecord {
            path,
            class_name,
            archetype,
            is_archetype: object.flags & 0x400 != 0,
            is_class_default: object.name.starts_with("Default__"),
            properties,
            property_error,
        })
    }

    fn properties(&mut self, object: &Object) -> Result<BTreeMap<String, Value>, String> {
        let data = self.read(object.offset, object.offset + object.size, MAX_EXPORT_DATA)?;
        let mut position = 4;
        // These reviewed component classes serialize TemplateOwnerClass before
        // NetIndex. Subobjects inside a class default additionally carry an FName.
        // A class-default object itself uses UObject's ordinary prefix.
        let class = self.resolve(object.class, 0)?;
        if object.flags & 0x200 == 0
            && matches!(
                class.as_deref(),
                Some(
                    "Engine.SkeletalMeshComponent"
                        | "Engine.StaticMeshComponent"
                        | "Engine.ParticleSystemComponent"
                )
            )
        {
            self.resolve(i32_at(&data, 0)?, 0)?;
            position = 8;
            let mut outer = object.outer;
            for depth in 0..128 {
                if outer <= 0 {
                    break;
                }
                let parent = self
                    .exports
                    .get(outer as usize - 1)
                    .ok_or("Invalid component outer")?;
                if parent.flags & 0x200 != 0 {
                    self.fname(&data, 4)?;
                    position = 16;
                    break;
                }
                outer = parent.outer;
                if depth == 127 {
                    return Err("Component outer cycle/limit".into());
                }
            }
        }
        let mut properties = BTreeMap::new();
        for _ in 0..65536 {
            let name = self.fname(&data, position)?;
            position += 8;
            if name == "None" {
                return Ok(properties);
            }
            let kind = self.fname(&data, position)?;
            let size = u32_at(&data, position + 8)? as usize;
            let index = u32_at(&data, position + 12)?;
            position += 16;
            if matches!(kind.as_str(), "StructProperty" | "ByteProperty") {
                self.fname(&data, position)?;
                position += 8;
            }
            let value = if kind == "BoolProperty" {
                let value = *data.get(position).ok_or("Truncated bool property")?;
                position += 1;
                if value > 1 {
                    return Err("Invalid bool property".into());
                }
                Some(json!(value != 0))
            } else {
                let end = position
                    .checked_add(size)
                    .ok_or("Property length overflow")?;
                let raw = data
                    .get(position..end)
                    .ok_or("Property crosses export boundary")?;
                position = end;
                match kind.as_str() {
                    "ByteProperty" if size == 8 => Some(json!(self.fname(raw, 0)?)),
                    "ByteProperty" if size == 1 => Some(json!(raw[0])),
                    "ObjectProperty" | "ComponentProperty" if size == 4 => {
                        Some(json!(self.resolve(i32_at(raw, 0)?, 0)?))
                    }
                    "IntProperty" if size == 4 => Some(json!(i32_at(raw, 0)?)),
                    "FloatProperty" if size == 4 => {
                        let number = f32::from_bits(u32_at(raw, 0)?);
                        if !number.is_finite() {
                            return Err("Non-finite property number".into());
                        }
                        Some(json!(number))
                    }
                    "NameProperty" if size == 8 => Some(json!(self.fname(raw, 0)?)),
                    "StrProperty" => {
                        let (value, consumed) = string_at(raw, 0, 65536)?;
                        if consumed != size {
                            return Err("String property length mismatch".into());
                        }
                        Some(json!(value))
                    }
                    "ArrayProperty" if name == "DroppedEquipmentTemplates" => {
                        let count = bounded_count(raw, 0)?;
                        if count.checked_mul(4).and_then(|value| value.checked_add(4)) != Some(size)
                        {
                            return Err("Object array length mismatch".into());
                        }
                        let mut values = Vec::with_capacity(count);
                        for index in 0..count {
                            values.push(self.resolve(i32_at(raw, 4 + index * 4)?, 0)?);
                        }
                        Some(json!(values))
                    }
                    "ByteProperty" | "ObjectProperty" | "ComponentProperty" | "IntProperty"
                    | "FloatProperty" | "NameProperty" => {
                        return Err(format!("Unexpected {kind} length"));
                    }
                    _ => None,
                }
            };
            if let Some(value) = value {
                let key = if index == 0 {
                    name
                } else {
                    format!("{name}[{index}]")
                };
                if properties.insert(key, value).is_some() {
                    return Err("Duplicate serialized property".into());
                }
            }
        }
        Err("Too many properties or missing terminator".into())
    }

    fn fname(&self, data: &[u8], offset: usize) -> Result<String, String> {
        let index = u32_at(data, offset)? as usize;
        let number = u32_at(data, offset + 4)?;
        let name = self
            .names
            .get(index)
            .ok_or("Name index outside package table")?;
        Ok(if number == 0 {
            name.clone()
        } else {
            format!("{name}_{}", number - 1)
        })
    }

    fn read(&mut self, start: usize, end: usize, limit: usize) -> Result<Vec<u8>, String> {
        if end < start || end > self.logical_size || end - start > limit {
            return Err("Logical package read outside bounds".into());
        }
        if self.chunks.is_empty() {
            return self.raw.read(start, end - start);
        }
        let mut result = vec![0; end - start];
        let mut copied = 0;
        if start < self.header_size {
            let stop = end.min(self.header_size);
            result[..stop - start].copy_from_slice(&self.raw.read(start, stop - start)?);
            copied += stop - start;
        }
        for index in 0..self.chunks.len() {
            let chunk = self.chunks[index];
            let first = start.max(chunk.logical);
            let last = end.min(chunk.logical + chunk.size);
            if first >= last {
                continue;
            }
            if self
                .chunk_cache
                .as_ref()
                .is_none_or(|(cached, _)| *cached != index)
            {
                self.chunk_cache = None;
                self.chunk_cache = Some((index, self.decode_chunk(chunk)?));
            }
            let decoded = &self.chunk_cache.as_ref().ok_or("Missing decoded chunk")?.1;
            result[first - start..last - start]
                .copy_from_slice(&decoded[first - chunk.logical..last - chunk.logical]);
            copied += last - first;
        }
        if copied != result.len() {
            return Err("Unmapped logical package range".into());
        }
        Ok(result)
    }

    fn decode_chunk(&mut self, chunk: Chunk) -> Result<Vec<u8>, String> {
        let bytes = self.raw.read(chunk.physical, chunk.compressed)?;
        let raw = bytes.as_slice();
        if u32_at(raw, 0)? != SIGNATURE || u32_at(raw, 12)? as usize != chunk.size {
            return Err("Invalid compression block header".into());
        }
        let block_size = u32_at(raw, 4)? as usize;
        if block_size == 0 || block_size > MAX_CHUNK {
            return Err("Invalid compression block size".into());
        }
        let blocks = chunk.size.div_ceil(block_size);
        let mut position = 16 + blocks * 8;
        if position > raw.len() {
            return Err("Truncated compression block table".into());
        }
        let mut output = Vec::with_capacity(chunk.size);
        for index in 0..blocks {
            let compressed = u32_at(raw, 16 + index * 8)? as usize;
            let decoded = u32_at(raw, 20 + index * 8)? as usize;
            if decoded > block_size || decoded > chunk.size - output.len() {
                return Err("Invalid decompressed block size".into());
            }
            let end = position
                .checked_add(compressed)
                .ok_or("Compression block overflow")?;
            output.extend(lzo::decompress(
                raw.get(position..end).ok_or("Truncated compressed block")?,
                decoded,
            )?);
            position = end;
        }
        if output.len() != chunk.size || position != raw.len() {
            return Err("Compressed chunk size mismatch".into());
        }
        Ok(output)
    }
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32, String> {
    let end = offset.checked_add(4).ok_or("Package offset overflow")?;
    Ok(u32::from_le_bytes(
        data.get(offset..end)
            .ok_or("Truncated package field")?
            .try_into()
            .map_err(|_| "Invalid field width")?,
    ))
}

fn i32_at(data: &[u8], offset: usize) -> Result<i32, String> {
    Ok(u32_at(data, offset)? as i32)
}

fn bounded_count(data: &[u8], offset: usize) -> Result<usize, String> {
    let count = u32_at(data, offset)? as usize;
    if count > MAX_RECORDS {
        return Err("Package record count exceeds parser limit".into());
    }
    Ok(count)
}

fn string_at(data: &[u8], offset: usize, limit: usize) -> Result<(String, usize), String> {
    let count = i32_at(data, offset)?;
    if count == 0 {
        return Ok((String::new(), 4));
    }
    let units = count.unsigned_abs() as usize;
    if units >= limit {
        return Err("Package string exceeds parser limit".into());
    }
    let bytes = units
        .checked_mul(if count < 0 { 2 } else { 1 })
        .ok_or("String length overflow")?;
    let raw = data
        .get(offset + 4..offset + 4 + bytes)
        .ok_or("Truncated package string")?;
    let value = if count < 0 {
        let units: Vec<_> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        if units.last() != Some(&0) {
            return Err("Unterminated UTF-16 string".into());
        }
        String::from_utf16(&units[..units.len() - 1]).map_err(|_| "Invalid UTF-16 string")?
    } else {
        if raw.last() != Some(&0) {
            return Err("Unterminated ANSI string".into());
        }
        raw[..raw.len() - 1]
            .iter()
            .map(|byte| char::from(*byte))
            .collect()
    };
    Ok((value, 4 + bytes))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

    struct DiskFixture(std::path::PathBuf);

    impl DiskFixture {
        fn new(bytes: &[u8]) -> Self {
            let directory =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/catalog-tests");
            std::fs::create_dir_all(&directory).unwrap();
            let path = directory.join(format!(
                "stream-{}-{}.upk",
                std::process::id(),
                FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::write(&path, bytes).unwrap();
            Self(path)
        }
    }

    impl Drop for DiskFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    pub(crate) fn empty_package() -> Vec<u8> {
        let mut data = vec![0; 128];
        data[..4].copy_from_slice(&SIGNATURE.to_le_bytes());
        data[4..8].copy_from_slice(&(678_u32 | (2 << 16)).to_le_bytes());
        data[12..21].copy_from_slice(b"\x05\0\0\0None\0");
        for offset in [29, 37, 45, 49] {
            data[offset..offset + 4].copy_from_slice(&128_u32.to_le_bytes());
        }
        data
    }

    /// Synthetic UE tables: no shipped bytes or game assets are used in tests.
    pub(crate) fn fixture(
        export_name: &str,
        class_package: &str,
        class_name: &str,
        definition: bool,
    ) -> Vec<u8> {
        let names = ["None", class_package, class_name, export_name];
        let mut data = empty_package();
        for name in names {
            data.extend_from_slice(&((name.len() + 1) as u32).to_le_bytes());
            data.extend_from_slice(name.as_bytes());
            data.push(0);
            data.extend_from_slice(&[0; 8]);
        }
        let imports = data.len();
        for (outer, name) in [(0_i32, 1_u32), (-1, 2)] {
            data.extend_from_slice(&[0; 16]);
            data.extend_from_slice(&outer.to_le_bytes());
            data.extend_from_slice(&name.to_le_bytes());
            data.extend_from_slice(&[0; 4]);
        }
        let exports = data.len();
        let mut export = vec![0; 68];
        export[0..4].copy_from_slice(&(if definition { 0_i32 } else { -2 }).to_le_bytes());
        export[4..8].copy_from_slice(&(if definition { -2_i32 } else { 0 }).to_le_bytes());
        export[12..16].copy_from_slice(&3_u32.to_le_bytes());
        export[24..28].copy_from_slice(&0x400_u32.to_le_bytes());
        export[32..36].copy_from_slice(&12_u32.to_le_bytes());
        export[36..40].copy_from_slice(&((exports + 68) as u32).to_le_bytes());
        data.extend(export);
        let depends = data.len();
        data.extend_from_slice(&[0; 12]); // net index followed by None property terminator
        for (offset, value) in [
            (25, 4),
            (29, 128),
            (33, 1),
            (37, exports),
            (41, 2),
            (45, imports),
            (49, depends),
        ] {
            data[offset..offset + 4].copy_from_slice(&(value as u32).to_le_bytes());
        }
        data
    }

    fn compressed_fixture(data: &[u8]) -> Vec<u8> {
        let literals = &data[128..];
        let mut compressed = Vec::new();
        if literals.len() <= 238 {
            compressed.push((literals.len() + 17) as u8);
        } else {
            compressed.push(0);
            let mut extended = literals.len() - 18;
            while extended > 255 {
                compressed.push(0);
                extended -= 255;
            }
            compressed.push(extended as u8);
        }
        compressed.extend_from_slice(literals);
        compressed.extend_from_slice(&[17, 0, 0]);
        let mut output = data[..128].to_vec();
        for (offset, value) in [
            (97, 2),
            (101, 1),
            (105, 128),
            (109, literals.len()),
            (113, 128),
            (117, compressed.len() + 24),
        ] {
            output[offset..offset + 4].copy_from_slice(&(value as u32).to_le_bytes());
        }
        for value in [
            SIGNATURE,
            131072,
            compressed.len() as u32,
            literals.len() as u32,
            compressed.len() as u32,
            literals.len() as u32,
        ] {
            output.extend_from_slice(&value.to_le_bytes());
        }
        output.extend(compressed);
        output
    }

    #[test]
    fn streamed_and_memory_readers_match_compressed_and_plain_exports() {
        let plain = fixture("MissionPickup", "Unreviewed", "MissionActor", false);
        for bytes in [plain.clone(), compressed_fixture(&plain)] {
            let fixture = DiskFixture::new(&bytes);
            let stem = fixture.0.file_stem().unwrap().to_str().unwrap();
            let mut memory = Package::open(&fixture.0).unwrap();
            let mut streamed = Package::open_streamed(&fixture.0).unwrap();
            assert!(memory.bytes().is_some());
            assert!(streamed.bytes().is_none());
            assert_eq!(memory.classes().unwrap(), streamed.classes().unwrap());
            assert_eq!(
                memory
                    .inspect_export(&format!("{stem}.MissionPickup"))
                    .unwrap(),
                streamed
                    .inspect_export(&format!(
                        "{}.MissionPickup",
                        fixture.0.file_stem().unwrap().to_str().unwrap()
                    ))
                    .unwrap()
            );
            assert!(
                streamed.paths.len() <= 4,
                "Selected inspection must not resolve unrelated exports"
            );
        }
    }

    #[test]
    fn streamed_reader_rejects_truncated_files_and_out_of_range_reads() {
        let plain = fixture("MissionPickup", "Unreviewed", "MissionActor", false);
        let fixture = DiskFixture::new(&plain);
        let mut streamed = Package::open_streamed(&fixture.0).unwrap();
        let writer = std::fs::OpenOptions::new()
            .write(true)
            .open(&fixture.0)
            .unwrap();
        writer.set_len((plain.len() - 4) as u64).unwrap();
        let selected = streamed
            .inspect_export(&format!(
                "{}.MissionPickup",
                fixture.0.file_stem().unwrap().to_str().unwrap()
            ))
            .unwrap();
        assert!(selected.property_error.unwrap().contains("Truncated"));
        assert!(streamed.raw.read(usize::MAX, 1).is_err());
        assert!(streamed.raw.read(0, MAX_TABLE + 1).is_err());
        drop(streamed);
        assert!(Package::open_streamed(&fixture.0).is_err());
    }

    #[test]
    fn reads_synthetic_class_graph_and_candidate_export() {
        let definition = fixture("NewEquipment", "UDKGame", "HeroEquipment", true);
        let classes = Package::parse(definition, "Other")
            .unwrap()
            .classes()
            .unwrap();
        assert_eq!(classes[0].path, "Other.NewEquipment");
        assert_eq!(classes[0].parent.as_deref(), Some("UDKGame.HeroEquipment"));
        let graph = classes
            .into_iter()
            .map(|class| (class.path, class.parent))
            .collect();
        let object = fixture("Template", "Other", "NewEquipment", false);
        let exports = Package::parse(object, "Items")
            .unwrap()
            .candidates(&graph)
            .unwrap();
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].class_name, "Other.NewEquipment");
        assert!(exports[0].is_archetype);
        assert!(exports[0].property_error.is_none());
    }

    #[test]
    fn component_prefixes_and_component_properties_are_bounded() {
        for (class_default, default_outer, prefix) in
            [(false, false, 8), (true, false, 4), (false, true, 16)]
        {
            let mut package = Package::parse(
                fixture("Mesh", "Engine", "SkeletalMeshComponent", false),
                "Test",
            )
            .unwrap();
            let mut object = package.exports[0].clone();
            if class_default {
                object.flags |= 0x200;
            }
            if default_outer {
                let mut parent = object.clone();
                parent.name = "Default__Owner".into();
                parent.flags |= 0x200;
                package.exports.push(parent);
                object.outer = 2;
            }
            let property = package.names.len() as u32;
            package
                .names
                .extend(["MeshLink".into(), "ComponentProperty".into()]);
            let mut body = vec![0; prefix];
            for word in [property, 0, property + 1, 0, 4, 0, 0, 0, 0] {
                body.extend_from_slice(&word.to_le_bytes());
            }
            let Input::Memory(bytes) = &mut package.raw else {
                panic!("memory fixture")
            };
            object.offset = bytes.len();
            object.size = body.len();
            bytes.extend(body);
            package.logical_size = bytes.len();
            let values = package.properties(&object).unwrap();
            assert_eq!(values.get("MeshLink"), Some(&Value::Null));
            object.size = prefix + 3;
            assert!(package.properties(&object).is_err());
        }
    }

    #[test]
    fn ordinary_roots_and_nested_exports_keep_source_package_identity() {
        for root in ["MapA", "MapB"] {
            let mut package =
                Package::parse(fixture("Weapon", "Core", "Object", false), root).unwrap();
            let mut component = package.exports[0].clone();
            component.outer = 1;
            component.name = "Mesh".into();
            package.exports.push(component);
            assert_eq!(
                package.resolve(1, 0).unwrap().unwrap(),
                format!("{root}.Weapon")
            );
            assert_eq!(
                package.resolve(2, 0).unwrap().unwrap(),
                format!("{root}.Weapon.Mesh")
            );
            assert_eq!(package.resolve(-1, 0).unwrap().as_deref(), Some("Core"));
            assert_eq!(
                package.resolve(-2, 0).unwrap().as_deref(),
                Some("Core.Object")
            );
        }
    }

    #[test]
    fn forced_export_roots_preserve_cooked_logical_names() {
        let mut bytes = fixture("LogicalPackage", "Core", "Package", false);
        let export = u32_at(&bytes, 37).unwrap() as usize;
        bytes[export + 40..export + 44].copy_from_slice(&1_u32.to_le_bytes());
        let mut package = Package::parse(bytes, "Startup_INT").unwrap();
        let mut child = package.exports[0].clone();
        child.outer = 1;
        child.name = "Item".into();
        package.exports.push(child);
        assert_eq!(
            package.resolve(1, 0).unwrap().as_deref(),
            Some("LogicalPackage")
        );
        assert_eq!(
            package.resolve(2, 0).unwrap().as_deref(),
            Some("LogicalPackage.Item")
        );
    }

    #[test]
    fn selected_export_reads_actual_unknown_class_and_rejects_other_paths() {
        let bytes = fixture("MissionPickup", "Unreviewed", "MissionActor", false);
        let mut package = Package::parse(bytes, "Items").unwrap();
        assert!(package.candidates(&BTreeMap::new()).unwrap().is_empty());
        let selected = package.inspect_export("Items.MissionPickup").unwrap();
        assert_eq!(selected.class_name, "Unreviewed.MissionActor");
        assert!(selected.is_archetype);
        assert!(selected.property_error.is_none());
        assert_eq!(
            package.inspect_export("items.missionpickup").unwrap(),
            selected
        );
        for path in [
            "",
            "Pickup",
            "MissionPickup",
            "Other.MissionPickup",
            "Items.MissionPickup.Other",
        ] {
            assert!(package.inspect_export(path).is_err());
        }
        package.exports.push(package.exports[0].clone());
        assert!(
            package
                .inspect_export("Items.MissionPickup")
                .unwrap_err()
                .contains("ambiguous")
        );
    }

    #[test]
    fn validates_empty_tables_and_format_gate() {
        let data = empty_package();
        assert!(
            Package::parse(data.clone(), "Test")
                .unwrap()
                .classes()
                .unwrap()
                .is_empty()
        );
        let mut unknown = data.clone();
        unknown[4] = 1;
        assert!(Package::parse(unknown, "Test").is_err());
        for length in 0..105 {
            assert!(Package::parse(data[..length].to_vec(), "Test").is_err());
        }
    }

    #[test]
    fn rejects_bad_counts_strings_and_object_cycles() {
        let mut data = empty_package();
        data[25..29].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Package::parse(data, "Test").is_err());
        assert!(string_at(&[2, 0, 0, 0, b'x', b'y'], 0, 10).is_err());
        let mut package = Package::parse(empty_package(), "Test").unwrap();
        package.exports.push(Object {
            outer: 1,
            name: "Cycle".into(),
            class: 0,
            parent: 0,
            archetype: 0,
            flags: 0,
            export_flags: 0,
            offset: 0,
            size: 0,
        });
        assert!(package.resolve(1, 0).is_err());
        assert!(package.resolve(i32::MIN, 0).is_err());
    }
}
