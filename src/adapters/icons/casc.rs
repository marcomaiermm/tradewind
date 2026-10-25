use crate::core::Game;
use anyhow::{Context, Result, bail, ensure};
use cascette_client_storage::{BuildInfoFile, index::IndexManager};
use cascette_crypto::{ContentKey, EncodingKey};
use cascette_formats::{config::BuildConfig, encoding::EncodingFile, root::RootFile};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const MANIFEST_LIMIT: usize = 256 * 1024 * 1024;
const ICON_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Build {
    pub root: PathBuf,
    pub product: &'static str,
    pub key: String,
}
impl Build {
    pub fn read(root: &Path, game: Game) -> Result<Self> {
        let source = read_bounded(&root.join(".build.info"), 1024 * 1024)?;
        let info = BuildInfoFile::parse_str(std::str::from_utf8(&source)?)?;
        let product = match game {
            Game::Retail => "wow",
            Game::ClassicEra => "wow_classic_era",
            Game::Progression => "wow_classic",
            Game::Anniversary => "wow_anniversary",
            Game::Forever => bail!("Unsupported WoW product"),
        };
        let entry = info
            .entries()
            .into_iter()
            .find(|e| e.is_active() && e.product() == Some(product))
            .context("Requested WoW product is not installed")?;
        let key = entry.build_key().context("Missing build key")?.to_owned();
        validate_key(&key)?;
        Ok(Self {
            root: root.to_owned(),
            product,
            key,
        })
    }
    fn config_path(&self) -> PathBuf {
        self.root
            .join("Data/config")
            .join(&self.key[..2])
            .join(&self.key[2..4])
            .join(&self.key)
    }
}
fn validate_key(key: &str) -> Result<()> {
    ensure!(
        key.len() == 32 && key.bytes().all(|c| c.is_ascii_hexdigit()),
        "Invalid CASC key"
    );
    Ok(())
}
pub(super) struct Reader {
    pub build: Build,
    indices: IndexManager,
    files: BTreeMap<u32, (ContentKey, Vec<EncodingKey>)>,
}
impl Reader {
    pub fn open(build: Build, wanted: &[u32]) -> Result<Self> {
        let data = build.root.join("Data");
        // Physical archive addresses live in the shared v7 index. The product's v8
        // residency index is a different format. Product isolation comes from its build root.
        let indices = load_indices(&data.join("data"))?;
        let config = BuildConfig::parse(std::io::Cursor::new(read_bounded(
            &build.config_path(),
            1024 * 1024,
        )?))
        .map_err(|e| anyhow::anyhow!("Invalid build config: {e}"))?;
        let encoding = config.encoding().context("Missing encoding manifest")?;
        let key = EncodingKey::from_hex(
            encoding
                .encoding_key
                .as_deref()
                .context("Missing encoding key")?,
        )?;
        let mut reader = Self {
            build,
            indices,
            files: BTreeMap::new(),
        };
        let encoding_data = reader.read_key(key, MANIFEST_LIMIT)?;
        ensure!(
            ContentKey::from_data(&encoding_data) == ContentKey::from_hex(&encoding.content_key)?,
            "Encoding manifest checksum mismatch"
        );
        let encoding = EncodingFile::parse(&encoding_data)?;
        let root_key = ContentKey::from_hex(config.root().context("Missing root key")?)?;
        let root_data = reader.read_content(
            root_key,
            &encoding.find_all_encodings(&root_key),
            MANIFEST_LIMIT,
        )?;
        let root = RootFile::parse(&root_data)?;
        let wanted: HashSet<_> = wanted.iter().copied().collect();
        for block in root.blocks {
            for record in block.records {
                let id = record.file_data_id.0;
                if wanted.contains(&id) && !reader.files.contains_key(&id) {
                    let keys: Vec<_> = encoding
                        .find_all_encodings(&record.content_key)
                        .into_iter()
                        .filter(|key| reader.indices.lookup(key).is_some())
                        .collect();
                    if !keys.is_empty() {
                        reader.files.insert(id, (record.content_key, keys));
                    }
                }
            }
        }
        Ok(reader)
    }
    pub fn read(&self, id: u32) -> Result<Vec<u8>> {
        let (content, keys) = self.files.get(&id).context("Icon not present locally")?;
        self.read_content(*content, keys, ICON_LIMIT)
    }
    fn read_content(
        &self,
        content: ContentKey,
        keys: &[EncodingKey],
        limit: usize,
    ) -> Result<Vec<u8>> {
        for key in keys {
            if let Ok(data) = self.read_key(*key, limit)
                && ContentKey::from_data(&data) == content
            {
                return Ok(data);
            }
        }
        bail!("Local CASC content missing or invalid")
    }
    fn read_key(&self, key: EncodingKey, limit: usize) -> Result<Vec<u8>> {
        let entry = self
            .indices
            .lookup(&key)
            .context("CASC encoding not available locally")?;
        ensure!(
            entry.size as usize <= limit + 1024 * 1024,
            "CASC entry exceeds limit"
        );
        let path = self
            .build
            .root
            .join("Data/data")
            .join(format!("data.{:03}", entry.archive_id()));
        // Positioned file reads also survive Battle.net replacing or truncating an archive.
        // Keeping an mmap of a changing installation could cause SIGBUS.
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(entry.archive_offset() as u64))?;
        let mut raw = vec![0u8; entry.size as usize];
        file.read_exact(&mut raw)?;
        let blte = raw
            .get(30..)
            .filter(|bytes| bytes.starts_with(b"BLTE"))
            .context("Invalid local CASC header")?;
        decode_blte(blte, limit)
    }
}
fn load_indices(path: &Path) -> Result<IndexManager> {
    let mut latest: BTreeMap<u8, (u32, PathBuf)> = BTreeMap::new();
    for file in fs::read_dir(path)? {
        let file = file?;
        let name = file.file_name();
        let name = name.to_string_lossy();
        if name.len() != 14
            || !name.ends_with(".idx")
            || !name.as_bytes()[..10].iter().all(|b| b.is_ascii_hexdigit())
        {
            continue;
        }
        let bucket = u8::from_str_radix(&name[..2], 16)?;
        let generation = u32::from_str_radix(&name[2..10], 16)?;
        if bucket < 16 && latest.get(&bucket).is_none_or(|(old, _)| generation > *old) {
            latest.insert(bucket, (generation, file.path()));
        }
    }
    let mut index = IndexManager::new(path);
    for (bucket, (_, path)) in latest {
        let file = read_bounded(&path, 64 * 1024 * 1024)?;
        ensure!(
            file.len() >= 40 && file[8..10] == [7, 0] && file[12..16] == [4, 5, 9, 30],
            "Unsupported CASC index"
        );
        let size = u32::from_le_bytes(file[32..36].try_into()?) as usize;
        ensure!(
            size <= file.len() - 40 && size.is_multiple_of(18),
            "Invalid CASC index size"
        );
        index.load_index(bucket, &path)?;
    }
    Ok(index)
}
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut data)?;
    ensure!(data.len() <= limit, "CASC file exceeds limit");
    Ok(data)
}
// Read only the unencrypted raw/zlib modes needed by local public item textures.
// Every decompression is bounded independently of the sizes claimed by the archive.
fn decode_blte(data: &[u8], limit: usize) -> Result<Vec<u8>> {
    ensure!(data.len() >= 9 && data.starts_with(b"BLTE"), "Invalid BLTE");
    let header = u32::from_be_bytes(data[4..8].try_into()?) as usize;
    let mut output = Vec::new();
    if header == 0 {
        decode_chunk(&data[8..], &mut output, limit)?;
    } else {
        ensure!(data.len() >= 12 && data[8] == 15, "Unsupported BLTE header");
        let count = u32::from_be_bytes([0, data[9], data[10], data[11]]) as usize;
        ensure!(
            count > 0 && header == 12 + count * 24 && header <= data.len(),
            "Invalid BLTE chunk table"
        );
        let mut position = header;
        for chunk in data[12..header].as_chunks::<24>().0 {
            let size = u32::from_be_bytes(chunk[..4].try_into()?) as usize;
            let decoded = u32::from_be_bytes(chunk[4..8].try_into()?) as usize;
            ensure!(
                decoded <= limit.saturating_sub(output.len()),
                "BLTE exceeds limit"
            );
            let bytes = data
                .get(position..position.checked_add(size).context("BLTE overflow")?)
                .context("Truncated BLTE")?;
            ensure!(
                ContentKey::from_data(bytes).as_bytes() == &chunk[8..24],
                "BLTE checksum mismatch"
            );
            let before = output.len();
            decode_chunk(bytes, &mut output, limit)?;
            ensure!(output.len() - before == decoded, "BLTE size mismatch");
            position += size;
        }
        ensure!(position == data.len(), "Trailing BLTE data");
    }
    Ok(output)
}
fn decode_chunk(data: &[u8], output: &mut Vec<u8>, limit: usize) -> Result<()> {
    let (&mode, data) = data.split_first().context("Empty BLTE chunk")?;
    match mode {
        b'N' => {
            ensure!(
                data.len() <= limit.saturating_sub(output.len()),
                "BLTE exceeds limit"
            );
            output.extend_from_slice(data);
        }
        b'Z' => {
            let available = limit.saturating_sub(output.len());
            flate2::read::ZlibDecoder::new(data)
                .take(available as u64 + 1)
                .read_to_end(output)?;
            ensure!(output.len() <= limit, "BLTE exceeds limit");
        }
        _ => bail!("Encrypted or unsupported BLTE chunk"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn product_selection_never_falls_back_to_another_client() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".build.info"), "Active!DEC:1|Build Key!HEX:16|Product!STRING:0\n1|0123456789abcdef0123456789abcdef|wow\n").unwrap();
        assert_eq!(
            Build::read(dir.path(), Game::Retail).unwrap().product,
            "wow"
        );
        assert!(Build::read(dir.path(), Game::Anniversary).is_err());
        assert!(validate_key("../../etc/passwd").is_err());
    }
    #[test]
    fn blte_enforces_output_limits_and_chunk_checksums() {
        assert_eq!(decode_blte(b"BLTE\0\0\0\0Nabc", 3).unwrap(), b"abc");
        assert!(decode_blte(b"BLTE\0\0\0\0Nabc", 2).is_err());
        assert!(decode_blte(b"BLTE\0\0\0\0Eabc", 100).is_err());
        let compressed = b"BLTE\0\0\0\0Z\x78\x9c\x4b\x4c\x4a\x06\x00\x02\x4d\x01\x27";
        assert_eq!(decode_blte(compressed, 3).unwrap(), b"abc");
        assert!(decode_blte(compressed, 2).is_err());
        let mut bytes = b"BLTE\0\0\0\x24\x0f\0\0\x01".to_vec();
        bytes.extend(4u32.to_be_bytes());
        bytes.extend(3u32.to_be_bytes());
        bytes.extend(ContentKey::from_data(b"Nabc").as_bytes());
        bytes.extend(b"Nabc");
        assert_eq!(decode_blte(&bytes, 3).unwrap(), b"abc");
        bytes[36] ^= 1;
        assert!(decode_blte(&bytes, 3).is_err());
    }
}
