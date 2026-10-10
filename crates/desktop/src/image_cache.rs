use gpui::{
    App, Context, Entity, ImageCache, ImageCacheError, RenderImage, Resource, RetainAllImageCache,
    WeakEntity, Window,
};
use std::{collections::VecDeque, sync::Arc};

pub(crate) struct ArtworkImageCache {
    cache: Entity<RetainAllImageCache>,
    recent: VecDeque<(Resource, usize)>,
    failed: Vec<Resource>,
    capacity: usize,
    byte_capacity: usize,
    pending_image_bytes: usize,
    entity: WeakEntity<Self>,
    frame_resources: Vec<Resource>,
    cleanup_scheduled: bool,
}

impl ArtworkImageCache {
    pub fn new(
        capacity: usize,
        byte_capacity: usize,
        pending_image_bytes: usize,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            cache: RetainAllImageCache::new(cx),
            recent: VecDeque::new(),
            failed: Vec::new(),
            capacity: capacity.max(1),
            byte_capacity,
            pending_image_bytes,
            entity: cx.entity().downgrade(),
            frame_resources: Vec::new(),
            cleanup_scheduled: false,
        }
    }

    pub fn invalidate_failed(&mut self, window: &mut Window, cx: &mut App) {
        for resource in self.failed.drain(..) {
            self.cache
                .update(cx, |cache, cx| cache.remove(&resource, window, cx));
        }
    }

    fn finish_frame(&mut self, window: &mut Window, cx: &mut App) {
        self.cleanup_scheduled = false;
        let pinned = std::mem::take(&mut self.frame_resources);
        let evicted = trim_images(&mut self.recent, self.capacity, self.byte_capacity, &pinned);
        let changed = !evicted.is_empty();
        if changed {
            crate::diagnostics::record(&format!(
                "image cache frame-boundary trim: {} unused images",
                evicted.len()
            ));
        }
        for resource in evicted {
            self.failed.retain(|failed| failed != &resource);
            self.cache
                .update(cx, |cache, cx| cache.remove(&resource, window, cx));
        }
        // Discard cached scene fragments that could still reference released atlas tiles.
        if changed {
            window.refresh();
        }
    }
}

impl ImageCache for ArtworkImageCache {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let result = self
            .cache
            .update(cx, |cache, cx| cache.load(resource, window, cx));
        let bytes = match &result {
            Some(Ok(image)) => (0..image.frame_count())
                .filter_map(|index| image.as_bytes(index))
                .map(<[u8]>::len)
                .fold(0usize, usize::saturating_add),
            Some(Err(_)) => 0,
            None => self.pending_image_bytes,
        };
        touch_image(&mut self.recent, resource, bytes);
        if !self.frame_resources.contains(resource) {
            self.frame_resources.push(resource.clone());
        }
        if !self.cleanup_scheduled {
            self.cleanup_scheduled = true;
            let entity = self.entity.clone();
            // ImageCache::load runs while GPUI builds a scene. Removing an earlier
            // image here frees a texture that a later draw in the same frame needs
            // (DirectXAtlas::texture panics). Trim only at the next frame boundary,
            // and keep the entire frame's working set even if it exceeds the budget.
            window.on_next_frame(move |window, cx| {
                let _ = entity.update(cx, |cache, cx| cache.finish_frame(window, cx));
            });
        }
        if matches!(result, Some(Err(_))) && !self.failed.iter().any(|failed| failed == resource) {
            self.failed.push(resource.clone());
        }
        result
    }
}

fn touch_image(recent: &mut VecDeque<(Resource, usize)>, resource: &Resource, bytes: usize) {
    // Linear accounting follows the bounded viewport working set; index if profiling warrants it.
    if let Some(index) = recent.iter().position(|(item, _)| item == resource) {
        recent.remove(index);
    }
    recent.push_back((resource.clone(), bytes));
}

fn trim_images(
    recent: &mut VecDeque<(Resource, usize)>,
    capacity: usize,
    byte_capacity: usize,
    pinned: &[Resource],
) -> Vec<Resource> {
    let mut total_bytes = recent
        .iter()
        .map(|(_, bytes)| *bytes)
        .fold(0usize, usize::saturating_add);
    let mut evicted = Vec::new();
    // A visible working set can exceed the budget; evict it only after it leaves
    // the frame. Releasing a still-visible image is unsafe for GPUI's GPU atlas.
    while recent.len() > 1 && (recent.len() > capacity || total_bytes > byte_capacity) {
        if let Some(index) = recent
            .iter()
            .position(|(resource, _)| !pinned.contains(resource))
        {
            let (old, bytes) = recent.remove(index).unwrap();
            total_bytes = total_bytes.saturating_sub(bytes);
            evicted.push(old);
        } else {
            break;
        }
    }
    evicted
}

#[cfg(test)]
fn retain_image(
    recent: &mut VecDeque<(Resource, usize)>,
    resource: &Resource,
    bytes: usize,
    capacity: usize,
    byte_capacity: usize,
) -> Vec<Resource> {
    touch_image(recent, resource, bytes);
    trim_images(
        recent,
        capacity,
        byte_capacity,
        std::slice::from_ref(resource),
    )
}

#[cfg(test)]
#[test]
fn denser_frames_never_release_textures_already_used_by_that_frame() {
    use std::path::PathBuf;
    let first = Resource::from(PathBuf::from("cover-a.png"));
    let second = Resource::from(PathBuf::from("cover-b.png"));
    let third = Resource::from(PathBuf::from("cover-c.png"));
    let mut recent = VecDeque::new();
    touch_image(&mut recent, &first, 60);
    touch_image(&mut recent, &second, 60);
    touch_image(&mut recent, &third, 60);
    assert!(
        trim_images(
            &mut recent,
            1,
            100,
            &[first.clone(), second.clone(), third.clone()]
        )
        .is_empty()
    );
    assert_eq!(
        recent.len(),
        3,
        "a frame may temporarily exceed the cache budget"
    );
    assert_eq!(
        trim_images(&mut recent, 1, 100, std::slice::from_ref(&third)),
        [first, second]
    );
    assert_eq!(recent.len(), 1);
}

#[cfg(test)]
#[test]
fn image_retention_obeys_byte_and_entry_limits() {
    use std::path::PathBuf;

    let first = Resource::from(PathBuf::from("first.png"));
    let second = Resource::from(PathBuf::from("second.png"));
    let third = Resource::from(PathBuf::from("third.png"));
    let mut recent = VecDeque::new();
    assert!(retain_image(&mut recent, &first, 40, 3, 100).is_empty());
    assert!(retain_image(&mut recent, &second, 40, 3, 100).is_empty());
    assert!(retain_image(&mut recent, &first, 40, 3, 100).is_empty());
    assert_eq!(
        retain_image(&mut recent, &third, 40, 3, 100),
        std::slice::from_ref(&second)
    );
    assert_eq!(
        retain_image(&mut recent, &second, 0, 2, 100),
        std::slice::from_ref(&first)
    );
    assert_eq!(
        retain_image(&mut recent, &first, 200, 3, 100),
        [third, second]
    );
    assert_eq!(recent.front(), Some(&(first.clone(), 200)));
    assert!(retain_image(&mut recent, &first, 40, 3, 100).is_empty());
    assert_eq!(recent.len(), 1);
}
