use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use base64::Engine;
use image::{GenericImageView, ImageDecoder, ImageFormat, ImageReader};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const MAX_ORIGINAL_BYTES: usize = 25 * 1024 * 1024;
pub const MAX_ORIGINAL_PIXELS: u64 = 50_000_000;
pub const MAX_IMAGES: usize = 8;
pub const MAX_PROMPT_IMAGE_BYTES: usize = 5 * 1024 * 1024;
pub const CHUNK_BYTES: usize = 32 * 1024;
pub const MAX_NORMALIZED_IMAGE_BYTES: usize = 1_500_000;
pub const TRANSFER_TTL_MS: u64 = 5 * 60 * 1000;
const MAX_TRANSFERS: usize = 4;
const MAX_STORED_BYTES: usize = 100 * 1024 * 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MediaError {
    #[error("image exceeds the supported size")]
    Size,
    #[error("image format is not supported")]
    Format,
    #[error("image could not be decoded")]
    Decode,
    #[error("image dimensions are not supported")]
    Dimensions,
    #[error("image transfer is not available")]
    Unknown,
    #[error("image transfer limit reached")]
    Capacity,
    #[error("image chunk is out of order")]
    Offset,
    #[error("image transfer is incomplete or corrupt")]
    Integrity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptImage {
    bytes: Arc<[u8]>,
    mime_type: &'static str,
    width: u32,
    height: u32,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ImageDescriptor {
    pub byte_length: usize,
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
    pub sha256: String,
}

impl PromptImage {
    pub fn normalize(bytes: &[u8]) -> Result<Self, MediaError> {
        if bytes.is_empty() || bytes.len() > MAX_ORIGINAL_BYTES {
            return Err(MediaError::Size);
        }
        let format = image::guess_format(bytes).map_err(|_| MediaError::Format)?;
        if !matches!(
            format,
            ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
        ) {
            return Err(MediaError::Format);
        }
        let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(50_000);
        limits.max_image_height = Some(50_000);
        limits.max_alloc = Some(256 * 1024 * 1024);
        reader.limits(limits);
        let mut decoder = reader.into_decoder().map_err(|_| MediaError::Decode)?;
        let (width, height) = decoder.dimensions();
        let pixels = u64::from(width) * u64::from(height);
        if width < 8 || height < 8 || !(512..=MAX_ORIGINAL_PIXELS).contains(&pixels) {
            return Err(MediaError::Dimensions);
        }
        let orientation = decoder.orientation().map_err(|_| MediaError::Decode)?;
        let mut decoded =
            image::DynamicImage::from_decoder(decoder).map_err(|_| MediaError::Decode)?;
        decoded.apply_orientation(orientation);
        let (width, height) = decoded.dimensions();
        let scale = (2000.0 / f64::from(width))
            .min(2000.0 / f64::from(height))
            .min((2_408_448.0 / (f64::from(width) * f64::from(height))).sqrt())
            .min(1.0);
        let width = (f64::from(width) * scale).floor() as u32;
        let height = (f64::from(height) * scale).floor() as u32;
        if width < 8 || height < 8 {
            return Err(MediaError::Dimensions);
        }
        if scale < 1.0 {
            decoded = decoded.resize_exact(width, height, image::imageops::FilterType::Triangle);
        }
        let mut encoded = Cursor::new(Vec::new());
        decoded
            .write_to(&mut encoded, ImageFormat::Png)
            .map_err(|_| MediaError::Decode)?;
        let mut normalized = encoded.into_inner();
        let mut mime_type = "image/png";
        if normalized.len() > MAX_NORMALIZED_IMAGE_BYTES {
            let rgba = decoded.to_rgba8();
            let rgb = image::RgbImage::from_fn(width, height, |x, y| {
                let pixel = rgba.get_pixel(x, y).0;
                let alpha = u32::from(pixel[3]);
                image::Rgb([0, 1, 2].map(|channel| {
                    ((u32::from(pixel[channel]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
                }))
            });
            for quality in [90, 80, 70, 60, 45] {
                normalized.clear();
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut normalized, quality)
                    .encode_image(&rgb)
                    .map_err(|_| MediaError::Decode)?;
                if normalized.len() <= MAX_NORMALIZED_IMAGE_BYTES {
                    break;
                }
            }
            mime_type = "image/jpeg";
        }
        if normalized.len() > MAX_NORMALIZED_IMAGE_BYTES {
            return Err(MediaError::Size);
        }
        let sha256 = format!("{:x}", Sha256::digest(&normalized));
        Ok(Self {
            bytes: normalized.into(),
            mime_type,
            width,
            height,
            sha256,
        })
    }

    pub fn descriptor(&self) -> ImageDescriptor {
        ImageDescriptor {
            byte_length: self.bytes.len(),
            mime_type: self.mime_type.into(),
            width: self.width,
            height: self.height,
            sha256: self.sha256.clone(),
        }
    }

    pub fn acp_block(&self) -> serde_json::Value {
        serde_json::json!({"type": "image", "mimeType": self.mime_type,
            "data": base64::engine::general_purpose::STANDARD.encode(&self.bytes)})
    }

    pub fn from_acp(block: &serde_json::Value) -> Result<Self, MediaError> {
        if block.get("type").and_then(serde_json::Value::as_str) != Some("image") {
            return Err(MediaError::Format);
        }
        let data = block
            .get("data")
            .and_then(serde_json::Value::as_str)
            .ok_or(MediaError::Format)?;
        if data.len() > MAX_ORIGINAL_BYTES.div_ceil(3) * 4 {
            return Err(MediaError::Size);
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| MediaError::Decode)?;
        Self::normalize(&bytes)
    }
}

#[derive(Debug, Default)]
pub struct TranscriptMedia {
    entries: HashMap<(String, String), PromptImage>,
    order: std::collections::VecDeque<(String, String)>,
    size: usize,
}

impl TranscriptMedia {
    pub fn descriptors(&self, session: &str) -> Vec<ImageDescriptor> {
        self.order
            .iter()
            .filter(|(id, _)| id == session)
            .filter_map(|key| self.entries.get(key).map(PromptImage::descriptor))
            .collect()
    }

    pub fn select(&self, session: &str, hashes: &[String]) -> Result<Vec<PromptImage>, MediaError> {
        if hashes.len() > MAX_IMAGES {
            return Err(MediaError::Size);
        }
        let mut selected = std::collections::HashSet::new();
        hashes
            .iter()
            .map(|hash| {
                if !selected.insert(hash) {
                    return Err(MediaError::Integrity);
                }
                self.entries
                    .get(&(session.to_owned(), hash.clone()))
                    .cloned()
                    .ok_or(MediaError::Unknown)
            })
            .collect()
    }

    pub fn remember(&mut self, session: &str, images: &[PromptImage]) {
        for image in images {
            let key = (session.to_owned(), image.sha256.clone());
            self.order.retain(|entry| entry != &key);
            if self.entries.insert(key.clone(), image.clone()).is_none() {
                self.size += image.bytes.len();
            }
            self.order.push_back(key);
            while self.size > MAX_STORED_BYTES || self.entries.len() > 128 {
                if let Some(old) = self.order.pop_front() {
                    if let Some(image) = self.entries.remove(&old) {
                        self.size -= image.bytes.len();
                    }
                } else {
                    break;
                }
            }
        }
    }

    pub fn chunk(
        &self,
        session: &str,
        sha256: &str,
        offset: usize,
    ) -> Result<serde_json::Value, MediaError> {
        let image = self
            .entries
            .get(&(session.to_owned(), sha256.to_owned()))
            .ok_or(MediaError::Unknown)?;
        if offset >= image.bytes.len() {
            return Err(MediaError::Offset);
        }
        let end = offset.saturating_add(CHUNK_BYTES).min(image.bytes.len());
        Ok(
            serde_json::json!({"outcome":"imageChunk","sha256":sha256,"offset":offset,
            "data":base64::engine::general_purpose::STANDARD.encode(&image.bytes[offset..end]),
            "nextOffset":end,"totalBytes":image.bytes.len()}),
        )
    }

    pub fn forget_session(&mut self, session: &str) {
        self.entries.retain(|(id, _), image| {
            if id == session {
                self.size -= image.bytes.len();
                false
            } else {
                true
            }
        });
        self.order.retain(|(id, _)| id != session);
    }
}

impl Serialize for PromptImage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.descriptor().serialize(serializer)
    }
}

pub fn validate_prompt_images(images: &[PromptImage]) -> Result<(), MediaError> {
    if images.len() > MAX_IMAGES
        || images.iter().map(|image| image.bytes.len()).sum::<usize>() > MAX_PROMPT_IMAGE_BYTES
    {
        return Err(MediaError::Size);
    }
    Ok(())
}

#[derive(Debug)]
struct Transfer {
    owner: String,
    session: String,
    expected_bytes: usize,
    expected_sha256: String,
    expires_at: u64,
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct ReadyImage {
    owner: String,
    session: String,
    expires_at: u64,
    image: PromptImage,
}

#[derive(Debug, Default)]
pub struct ImageTransfers {
    pending: HashMap<String, Transfer>,
    ready: HashMap<String, ReadyImage>,
}

impl ImageTransfers {
    fn expire(&mut self, now: u64) {
        self.pending.retain(|_, transfer| transfer.expires_at > now);
        self.ready.retain(|_, image| image.expires_at > now);
    }

    pub fn begin(
        &mut self,
        owner: &str,
        session: &str,
        expected_bytes: usize,
        expected_sha256: &str,
        now: u64,
    ) -> Result<String, MediaError> {
        self.expire(now);
        if expected_bytes == 0 || expected_bytes > MAX_ORIGINAL_BYTES {
            return Err(MediaError::Size);
        }
        if owner.is_empty() || session.is_empty() || owner.len() > 256 || session.len() > 256 {
            return Err(MediaError::Unknown);
        }
        if expected_sha256.len() != 64
            || !expected_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(MediaError::Integrity);
        }
        if self.pending.len() >= MAX_TRANSFERS
            || self
                .pending
                .values()
                .filter(|transfer| transfer.owner == owner)
                .count()
                >= 2
            || self
                .pending
                .values()
                .map(|transfer| transfer.expected_bytes)
                .sum::<usize>()
                + expected_bytes
                > MAX_STORED_BYTES
        {
            return Err(MediaError::Capacity);
        }
        let mut random = [0u8; 32];
        getrandom::fill(&mut random).map_err(|_| MediaError::Capacity)?;
        let id: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        self.pending.insert(
            id.clone(),
            Transfer {
                owner: owner.into(),
                session: session.into(),
                expected_bytes,
                expected_sha256: expected_sha256.into(),
                expires_at: now.saturating_add(TRANSFER_TTL_MS),
                bytes: Vec::new(),
            },
        );
        Ok(id)
    }

    pub fn append(
        &mut self,
        owner: &str,
        session: &str,
        id: &str,
        offset: usize,
        chunk: &[u8],
        now: u64,
    ) -> Result<usize, MediaError> {
        self.expire(now);
        let transfer = self
            .pending
            .get_mut(id)
            .filter(|transfer| transfer.owner == owner && transfer.session == session)
            .ok_or(MediaError::Unknown)?;
        if chunk.is_empty()
            || chunk.len() > CHUNK_BYTES
            || transfer.bytes.len().saturating_add(chunk.len()) > transfer.expected_bytes
        {
            return Err(MediaError::Size);
        }
        if transfer.bytes.len() != offset {
            return Err(MediaError::Offset);
        }
        transfer.bytes.extend_from_slice(chunk);
        Ok(transfer.bytes.len())
    }

    pub fn finish(
        &mut self,
        owner: &str,
        session: &str,
        id: &str,
        now: u64,
    ) -> Result<Vec<u8>, MediaError> {
        self.expire(now);
        let transfer = self
            .pending
            .get(id)
            .filter(|transfer| transfer.owner == owner && transfer.session == session)
            .ok_or(MediaError::Unknown)?;
        if transfer.bytes.len() != transfer.expected_bytes {
            return Err(MediaError::Integrity);
        }
        let transfer = self.pending.remove(id).ok_or(MediaError::Unknown)?;
        if format!("{:x}", Sha256::digest(&transfer.bytes)) != transfer.expected_sha256 {
            return Err(MediaError::Integrity);
        }
        Ok(transfer.bytes)
    }

    pub fn insert(
        &mut self,
        owner: &str,
        session: &str,
        image: PromptImage,
        now: u64,
    ) -> Result<String, MediaError> {
        self.expire(now);
        if self.ready.len() >= 64
            || self
                .ready
                .values()
                .filter(|entry| entry.owner == owner)
                .count()
                >= MAX_IMAGES
            || self
                .ready
                .values()
                .map(|entry| entry.image.bytes.len())
                .sum::<usize>()
                + image.bytes.len()
                > MAX_STORED_BYTES
        {
            return Err(MediaError::Capacity);
        }
        let mut random = [0u8; 32];
        getrandom::fill(&mut random).map_err(|_| MediaError::Capacity)?;
        let id: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        self.ready.insert(
            id.clone(),
            ReadyImage {
                owner: owner.into(),
                session: session.into(),
                expires_at: now.saturating_add(TRANSFER_TTL_MS),
                image,
            },
        );
        Ok(id)
    }

    pub fn resolve(
        &mut self,
        owner: Option<&str>,
        session: &str,
        ids: &[String],
        now: u64,
    ) -> Result<Vec<PromptImage>, MediaError> {
        self.expire(now);
        if ids.is_empty() || ids.len() > MAX_IMAGES {
            return Err(MediaError::Size);
        }
        let mut unique = std::collections::HashSet::new();
        let images = ids
            .iter()
            .map(|id| {
                if !unique.insert(id) {
                    return Err(MediaError::Integrity);
                }
                self.ready
                    .get(id)
                    .filter(|entry| {
                        entry.session == session && owner.is_none_or(|owner| entry.owner == owner)
                    })
                    .map(|entry| entry.image.clone())
                    .ok_or(MediaError::Unknown)
            })
            .collect::<Result<Vec<_>, _>>()?;
        validate_prompt_images(&images)?;
        Ok(images)
    }

    pub fn revoke(&mut self, owner: Option<&str>) {
        self.pending
            .retain(|_, transfer| owner.is_some_and(|owner| transfer.owner != owner));
        self.ready
            .retain(|_, image| owner.is_some_and(|owner| image.owner != owner));
    }

    pub fn discard(&mut self, owner: &str, id: &str) -> Result<(), MediaError> {
        if self.ready.get(id).is_some_and(|entry| entry.owner == owner) {
            self.ready.remove(id);
            return Ok(());
        }
        if self
            .pending
            .get(id)
            .is_some_and(|entry| entry.owner == owner)
        {
            self.pending.remove(id);
            return Ok(());
        }
        Err(MediaError::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbImage::from_pixel(width, height, image::Rgb([17, 39, 200]));
        let mut encoded = Cursor::new(Vec::new());
        image.write_to(&mut encoded, ImageFormat::Png).unwrap();
        encoded.into_inner()
    }

    #[test]
    fn decoding_corrects_mime_and_reencoding_discards_trailing_metadata() {
        let mut input = png(32, 32);
        input.extend_from_slice(b"location-secret-not-for-the-model");
        let normalized = PromptImage::normalize(&input).unwrap();
        let block = normalized.acp_block();
        assert_eq!(block["type"], "image");
        assert_eq!(block["mimeType"], "image/png");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(block["data"].as_str().unwrap())
            .unwrap();
        assert!(!bytes.windows(15).any(|window| window == b"location-secret"));
        assert_eq!(
            image::load_from_memory(&bytes).unwrap().dimensions(),
            (32, 32)
        );
        assert_eq!(
            normalized.descriptor().sha256,
            format!("{:x}", Sha256::digest(&bytes))
        );
    }

    #[test]
    fn invalid_small_large_and_unknown_images_are_rejected() {
        assert_eq!(
            PromptImage::normalize(b"<svg></svg>"),
            Err(MediaError::Format)
        );
        assert_eq!(PromptImage::normalize(&[]), Err(MediaError::Size));
        assert_eq!(
            PromptImage::normalize(&png(8, 8)),
            Err(MediaError::Dimensions)
        );
        assert_eq!(
            PromptImage::normalize(&png(7, 80)),
            Err(MediaError::Dimensions)
        );
        let mut truncated = png(32, 32);
        truncated.truncate(truncated.len() / 2);
        assert!(PromptImage::normalize(&truncated).is_err());
        assert_eq!(
            PromptImage::normalize(&vec![0; MAX_ORIGINAL_BYTES + 1]),
            Err(MediaError::Size)
        );
    }

    #[test]
    fn resize_stays_inside_upstream_pixel_and_byte_bounds() {
        let normalized = PromptImage::normalize(&png(2500, 2200)).unwrap();
        let descriptor = normalized.descriptor();
        assert!(descriptor.width <= 2000 && descriptor.height <= 2000);
        assert!(u64::from(descriptor.width) * u64::from(descriptor.height) <= 2_408_448);
        assert!(descriptor.byte_length <= MAX_NORMALIZED_IMAGE_BYTES);
    }

    #[test]
    fn upload_is_scoped_ordered_bounded_and_does_not_extend_deadline() {
        let mut transfers = ImageTransfers::default();
        let bytes = png(32, 32);
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let id = transfers
            .begin("one", "s", bytes.len(), &digest, 10)
            .unwrap();
        assert_eq!(
            transfers.append("two", "s", &id, 0, &bytes, 11),
            Err(MediaError::Unknown)
        );
        assert_eq!(
            transfers.append("one", "other", &id, 0, &bytes, 11),
            Err(MediaError::Unknown)
        );
        assert_eq!(
            transfers.append("one", "s", &id, 1, &bytes, 11),
            Err(MediaError::Offset)
        );
        assert_eq!(
            transfers.finish("one", "s", &id, 11),
            Err(MediaError::Integrity)
        );
        assert_eq!(
            transfers.append("one", "s", &id, 0, &bytes[..10], 12),
            Ok(10)
        );
        assert_eq!(
            transfers.append("one", "s", &id, 10, &bytes[10..], 10 + TRANSFER_TTL_MS),
            Err(MediaError::Unknown)
        );
    }

    #[test]
    fn duplicate_upload_digests_have_distinct_ids_and_bad_digest_is_terminal() {
        let mut transfers = ImageTransfers::default();
        let bytes = png(32, 32);
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let one = transfers
            .begin("one", "s", bytes.len(), &digest, 10)
            .unwrap();
        let two = transfers
            .begin("one", "s", bytes.len(), &digest, 10)
            .unwrap();
        assert_ne!(one, two);
        assert_eq!(
            transfers.begin("one", "s", bytes.len(), &digest, 10),
            Err(MediaError::Capacity)
        );
        let invalid = vec![0; bytes.len()];
        transfers.append("one", "s", &one, 0, &invalid, 11).unwrap();
        assert_eq!(
            transfers.finish("one", "s", &one, 11),
            Err(MediaError::Integrity)
        );
        assert_eq!(
            transfers.finish("one", "s", &one, 11),
            Err(MediaError::Unknown)
        );
        transfers.append("one", "s", &two, 0, &bytes, 11).unwrap();
        assert_eq!(transfers.finish("one", "s", &two, 11).unwrap(), bytes);
    }

    #[test]
    fn ready_images_enforce_ownership_session_duplicates_count_and_revocation() {
        let image = PromptImage::normalize(&png(32, 32)).unwrap();
        let mut transfers = ImageTransfers::default();
        let id = transfers.insert("one", "s", image.clone(), 0).unwrap();
        let ids = vec![id.clone()];
        assert_eq!(
            transfers.resolve(Some("two"), "s", &ids, 1),
            Err(MediaError::Unknown)
        );
        assert_eq!(
            transfers.resolve(Some("one"), "other", &ids, 1),
            Err(MediaError::Unknown)
        );
        assert_eq!(
            transfers.resolve(Some("one"), "s", &[id.clone(), id], 1),
            Err(MediaError::Integrity)
        );
        assert_eq!(
            transfers.resolve(Some("one"), "s", &ids, 1),
            Ok(vec![image.clone()])
        );
        transfers.revoke(Some("one"));
        assert_eq!(
            transfers.resolve(Some("one"), "s", &ids, 1),
            Err(MediaError::Unknown)
        );
        assert_eq!(
            validate_prompt_images(&vec![image; MAX_IMAGES + 1]),
            Err(MediaError::Size)
        );
    }
    #[test]
    fn publication_selection_cannot_read_another_session_or_silently_substitute_evicted_images() {
        let image = PromptImage::normalize(&png(32, 32)).unwrap();
        let hash = image.descriptor().sha256;
        let mut media = TranscriptMedia::default();
        media.remember("one", std::slice::from_ref(&image));
        assert_eq!(media.descriptors("one"), vec![image.descriptor()]);
        assert!(media.descriptors("two").is_empty());
        assert_eq!(
            media.select("one", std::slice::from_ref(&hash)).unwrap(),
            vec![image]
        );
        assert_eq!(
            media.select("two", std::slice::from_ref(&hash)),
            Err(MediaError::Unknown)
        );
        assert_eq!(
            media.select("one", &[hash.clone(), hash.clone()]),
            Err(MediaError::Integrity)
        );
        media.forget_session("one");
        assert_eq!(
            media.select("one", std::slice::from_ref(&hash)),
            Err(MediaError::Unknown)
        );
    }
}
