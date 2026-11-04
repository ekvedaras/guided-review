use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::code_source::{BlobInfo, CodeSource, FileStatus};
use crate::image_facts::{self, MAX_PREVIEW_BYTES, PixelSize};
use crate::pieces::ChangedFile;
use crate::pull_request::Sha;

/// Image bytes kept on disk under the blob id, which names the content, so a cached file is never stale.
#[derive(Debug, Clone)]
pub struct ImageCache {
    dir: PathBuf,
}

impl ImageCache {
    pub fn under(home: &std::path::Path) -> Self {
        Self {
            dir: home.join(".guided-review/cache/images"),
        }
    }

    fn read(&self, blob_id: &str) -> Option<Vec<u8>> {
        std::fs::read(self.dir.join(blob_id)).ok()
    }

    /// A cache that cannot be written only costs a second download later.
    fn write(&self, blob_id: &str, bytes: &[u8]) {
        static WRITES: AtomicU64 = AtomicU64::new(0);
        let is_plain_id = !blob_id.is_empty() && blob_id.chars().all(|c| c.is_ascii_hexdigit());
        if !is_plain_id || std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        // One blob can load on two threads at once (an image copied to two paths): each writes
        // its own temp file, so neither renames a file the other is still writing.
        let write = WRITES.fetch_add(1, Ordering::Relaxed);
        let unfinished = self
            .dir
            .join(format!("{blob_id}.{}.{write}.tmp", std::process::id()));
        let finished = std::fs::write(&unfinished, bytes)
            .and_then(|()| std::fs::rename(&unfinished, self.dir.join(blob_id)));
        if finished.is_err() {
            std::fs::remove_file(&unfinished).ok();
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedImage {
    pub bytes: Arc<[u8]>,
    /// `None` when the bytes are not an image the picture can be drawn from.
    pub pixels: Option<PixelSize>,
}

#[derive(Debug, Clone)]
pub enum ImageSide {
    Loaded(LoadedImage),
    TooLarge { file_bytes: u64 },
    Unreadable(String),
}

impl ImageSide {
    /// The facts row text of this side: `64×64, 117 KB`.
    pub fn facts(&self) -> String {
        match self {
            Self::Loaded(image) => {
                let file_size = file_size_text(image.bytes.len() as u64);
                match image.pixels {
                    Some(pixels) => format!("{pixels}, {file_size}"),
                    None => file_size,
                }
            }
            Self::TooLarge { file_bytes } => {
                format!("too large to preview, {}", file_size_text(*file_bytes))
            }
            Self::Unreadable(_) => "could not be read".to_owned(),
        }
    }

    fn file_bytes(&self) -> Option<u64> {
        match self {
            Self::Loaded(image) => Some(image.bytes.len() as u64),
            Self::TooLarge { file_bytes } => Some(*file_bytes),
            Self::Unreadable(_) => None,
        }
    }
}

fn file_size_text(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * KB;
    match bytes {
        0..KB => format!("{bytes} B"),
        KB..MB => format!("{:.0} KB", bytes as f64 / KB as f64),
        _ => format!("{:.1} MB", bytes as f64 / MB as f64),
    }
}

/// An image file of the PR as the reviewer sees it: base left, head right.
#[derive(Debug, Clone)]
pub struct ImagePreview {
    pub path: String,
    pub is_svg: bool,
    status: FileStatus,
    base_sha: Sha,
    head_sha: Sha,
    /// `None` for an added image.
    pub base: Option<ImageSide>,
    /// `None` for a deleted image.
    pub head: Option<ImageSide>,
}

impl ImagePreview {
    pub(super) fn is_of(&self, base: &Sha, head: &Sha) -> bool {
        self.base_sha == *base && self.head_sha == *head
    }

    pub fn mark(&self) -> Option<&'static str> {
        match self.status {
            FileStatus::Added => Some("added"),
            FileStatus::Deleted => Some("deleted"),
            FileStatus::Modified | FileStatus::Renamed => None,
        }
    }

    /// `117 KB → 48 KB`, only when both sides have a size.
    pub fn size_change(&self) -> Option<String> {
        let (base, head) = (
            self.base.as_ref()?.file_bytes()?,
            self.head.as_ref()?.file_bytes()?,
        );
        Some(format!(
            "{} → {}",
            file_size_text(base),
            file_size_text(head)
        ))
    }
}

/// What to load for one image file, owned so it can run on a background thread.
#[derive(Debug, Clone)]
pub struct ImagePreviewRequest {
    path: String,
    base_path: String,
    status: FileStatus,
    base: Sha,
    head: Sha,
}

impl ImagePreviewRequest {
    pub(super) fn for_file(file: &ChangedFile, base: &Sha, head: &Sha) -> Self {
        Self {
            path: file.path.as_str().to_owned(),
            base_path: file
                .old_path
                .as_ref()
                .unwrap_or(&file.path)
                .as_str()
                .to_owned(),
            status: file.status,
            base: base.clone(),
            head: head.clone(),
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// Blocking. Reads each side through the cache, and never fetches a side above the size limit.
    pub fn load(&self, source: &dyn CodeSource, cache: &ImageCache) -> ImagePreview {
        let has_base = self.status != FileStatus::Added;
        let has_head = self.status != FileStatus::Deleted;
        ImagePreview {
            path: self.path.clone(),
            is_svg: image_facts::is_svg_path(&self.path),
            status: self.status,
            base_sha: self.base.clone(),
            head_sha: self.head.clone(),
            base: has_base.then(|| load_side(source, cache, &self.base, &self.base_path)),
            head: has_head.then(|| load_side(source, cache, &self.head, &self.path)),
        }
    }
}

fn load_side(source: &dyn CodeSource, cache: &ImageCache, sha: &Sha, path: &str) -> ImageSide {
    let info = match source.blob_info_at(sha, path) {
        Ok(info) => info,
        Err(error) => return ImageSide::Unreadable(format!("{error:#}")),
    };
    if info.size > MAX_PREVIEW_BYTES {
        return ImageSide::TooLarge {
            file_bytes: info.size,
        };
    }
    match bytes_of(source, cache, sha, path, &info) {
        Ok(bytes) => ImageSide::Loaded(LoadedImage {
            pixels: image_facts::measure(path, &bytes),
            bytes,
        }),
        Err(error) => ImageSide::Unreadable(format!("{error:#}")),
    }
}

fn bytes_of(
    source: &dyn CodeSource,
    cache: &ImageCache,
    sha: &Sha,
    path: &str,
    info: &BlobInfo,
) -> anyhow::Result<Arc<[u8]>> {
    if let Some(bytes) = cache.read(&info.id) {
        return Ok(bytes.into());
    }
    let bytes = source.blob_at(sha, path)?;
    cache.write(&info.id, &bytes);
    Ok(bytes)
}
