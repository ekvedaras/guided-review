use std::collections::HashMap;
use std::sync::Arc;

use gpui::{App, Global, Image, ImageFormat};

use crate::github::GitHubAvatars;
use crate::viewer::{AvatarOutcome, AvatarPort, Avatars};

/// The comment authors' pictures for the whole app, so every PR window shares one download each.
pub struct AvatarPictures {
    avatars: Avatars,
    images: HashMap<String, Arc<Image>>,
    port: Arc<dyn AvatarPort>,
}

impl Global for AvatarPictures {}

impl AvatarPictures {
    pub fn install(cx: &mut App) {
        cx.set_global(Self {
            avatars: Avatars::default(),
            images: HashMap::new(),
            port: Arc::new(GitHubAvatars),
        });
    }

    /// The picture at `url`; `None` while it downloads, which starts on the first ask, or when it failed.
    pub fn picture(url: &str, cx: &mut App) -> Option<Arc<Image>> {
        let pictures = cx.try_global::<Self>()?;
        if let Some(image) = pictures.images.get(url) {
            return Some(Arc::clone(image));
        }
        let port = Arc::clone(&pictures.port);
        let request = cx.global_mut::<Self>().avatars.want(url)?;
        cx.spawn(async move |cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { request.run(port.as_ref()) })
                .await;
            cx.update(|cx| {
                cx.global_mut::<Self>().landed(outcome);
                cx.refresh_windows();
            });
        })
        .detach();
        None
    }

    fn landed(&mut self, outcome: AvatarOutcome) {
        let url = outcome.url().to_owned();
        self.avatars.landed(outcome);
        let Some(bytes) = self.avatars.get(&url) else {
            return;
        };
        let Some(format) = gpui_format(bytes) else {
            return;
        };
        self.images
            .insert(url, Arc::new(Image::from_bytes(format, bytes.to_vec())));
    }
}

fn gpui_format(bytes: &[u8]) -> Option<ImageFormat> {
    match image::guess_format(bytes).ok()? {
        image::ImageFormat::Png => Some(ImageFormat::Png),
        image::ImageFormat::Jpeg => Some(ImageFormat::Jpeg),
        image::ImageFormat::Gif => Some(ImageFormat::Gif),
        image::ImageFormat::WebP => Some(ImageFormat::Webp),
        _ => None,
    }
}
