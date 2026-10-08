use gpui::{
    App, Context, Entity, ImageCache, ImageCacheError, RenderImage, Resource, RetainAllImageCache,
    Window,
};
use std::{collections::VecDeque, sync::Arc};

pub(crate) struct ArtworkImageCache {
    cache: Entity<RetainAllImageCache>,
    recent: VecDeque<(Resource, usize)>,
    failed: Vec<Resource>,
    capacity: usize,
    byte_capacity: usize,
    pending_image_bytes: usize,
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
        }
    }

    pub fn invalidate_failed(&mut self, window: &mut Window, cx: &mut App) {
        for resource in self.failed.drain(..) {
            self.cache
                .update(cx, |cache, cx| cache.remove(&resource, window, cx));
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
        for old in retain_image(
            &mut self.recent,
            resource,
            bytes,
            self.capacity,
            self.byte_capacity,
        ) {
            self.failed.retain(|resource| resource != &old);
            self.cache
                .update(cx, |cache, cx| cache.remove(&old, window, cx));
        }
        if matches!(result, Some(Err(_))) && !self.failed.iter().any(|failed| failed == resource) {
            self.failed.push(resource.clone());
        }
        result
    }
}

fn retain_image(
    recent: &mut VecDeque<(Resource, usize)>,
    resource: &Resource,
    bytes: usize,
    capacity: usize,
    byte_capacity: usize,
) -> Vec<Resource> {
    // ponytail: linear accounting is bounded to 96 images; use an indexed cache if profiling warrants it.
    if let Some(index) = recent.iter().position(|(item, _)| item == resource) {
        recent.remove(index);
    }
    recent.push_back((resource.clone(), bytes));
    let mut total_bytes = recent
        .iter()
        .map(|(_, bytes)| *bytes)
        .fold(0usize, usize::saturating_add);
    let mut evicted = Vec::new();
    // ponytail: one oversized current image may exceed the budget; core dimension limits bound normal artwork.
    while recent.len() > 1 && (recent.len() > capacity || total_bytes > byte_capacity) {
        if let Some((old, bytes)) = recent.pop_front() {
            total_bytes = total_bytes.saturating_sub(bytes);
            evicted.push(old);
        }
    }
    evicted
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
