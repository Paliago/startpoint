use gpui::{
    App, Context, Entity, ImageCache, ImageCacheError, RenderImage, Resource, RetainAllImageCache,
    Window,
};
use std::{collections::VecDeque, sync::Arc};

pub(crate) struct ThumbnailCache {
    cache: Entity<RetainAllImageCache>,
    recent: VecDeque<Resource>,
    failed: Vec<Resource>,
}

impl ThumbnailCache {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            cache: RetainAllImageCache::new(cx),
            recent: VecDeque::new(),
            failed: Vec::new(),
        }
    }

    pub fn invalidate_failed(&mut self, window: &mut Window, cx: &mut App) {
        for resource in self.failed.drain(..) {
            self.cache
                .update(cx, |cache, cx| cache.remove(&resource, window, cx));
        }
    }
}

impl ImageCache for ThumbnailCache {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        // ponytail: linear recency lookup is bounded to 96 thumbnails; replace only if profiling warrants it.
        if let Some(index) = self.recent.iter().position(|item| item == resource) {
            self.recent.remove(index);
        }
        self.recent.push_back(resource.clone());
        while self.recent.len() > 96 {
            if let Some(old) = self.recent.pop_front() {
                self.failed.retain(|resource| resource != &old);
                self.cache
                    .update(cx, |cache, cx| cache.remove(&old, window, cx));
            }
        }
        let result = self
            .cache
            .update(cx, |cache, cx| cache.load(resource, window, cx));
        if matches!(result, Some(Err(_))) && !self.failed.iter().any(|failed| failed == resource) {
            self.failed.push(resource.clone());
        }
        result
    }
}
