use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use pdslib::{events::uri_set::UriSet, util::hashmap::HashSet};
use thread_local::ThreadLocal;

use crate::common_types::Uri;

static NEXT_LOCALIZER_ID: AtomicU64 = AtomicU64::new(0);

thread_local! {
    // Each entry is (owner_id, UriSet) to verify ownership after thread recycling
    static URISET_CACHE: RefCell<Vec<(u64, UriSet<Uri>)>> = const { RefCell::new(Vec::new()) };
}

/// pdslib is single-threaded, so it can make optimizations such as using Rc<>
/// instead of Arc<> in its UriSet.
/// This eval framework, however, is multi-threaded. One of things this allows
/// us to do is read the dataset once, and share the Queriers and Conversions
/// across all threads.
/// This poses a dilemma. We want to share these objects across threads, but
/// they must also hold a thread-local UriSet type from pdslib.
///
/// Solution:
/// This wrapper type hold a thread-safe HashSet<Uri>, and when requested, it
/// creates a thread-local UriSet<Uri> from it for that thread.
/// It then holds a different UriSet<> for each thread that requests it.
#[derive(Debug)]
pub struct UriSetLocalizer {
    id: u64,
    uri_set: HashSet<Uri>,
    local_cache: ThreadLocal<Cell<Option<usize>>>,
}

impl UriSetLocalizer {
    pub fn new(mut uri_set: HashSet<Uri>) -> Self {
        uri_set.shrink_to_fit();

        Self {
            id: NEXT_LOCALIZER_ID.fetch_add(1, Ordering::Relaxed),
            uri_set,
            local_cache: ThreadLocal::new(),
        }
    }

    pub fn get(&self) -> UriSet<Uri> {
        let cell = self.local_cache.get_or(|| Cell::new(None));

        // Check if cached index is valid AND belongs to this localizer
        let cached_index = cell.get();
        let valid_index = cached_index.filter(|&idx| {
            URISET_CACHE.with(|cache| {
                cache
                    .borrow()
                    .get(idx)
                    .is_some_and(|(owner_id, _)| *owner_id == self.id)
            })
        });

        let index = match valid_index {
            Some(idx) => idx,
            None => {
                // Need to create a new entry (first access or cache was
                // cleared/recycled due to thread reuse)
                let new_index = URISET_CACHE.with(|cache| {
                    let mut cache_borrow = cache.borrow_mut();
                    let new_uriset = UriSet {
                        uris: Rc::new(self.uri_set.clone()),
                    };
                    cache_borrow.push((self.id, new_uriset));
                    cache_borrow.len() - 1
                });
                cell.set(Some(new_index));
                new_index
            }
        };

        URISET_CACHE.with(|cache| cache.borrow()[index].1.clone())
    }

    fn inner_set(&self) -> &HashSet<Uri> {
        &self.uri_set
    }
}

#[derive(Debug, Clone)]
pub enum UriSetOrLocalizer {
    /// only used from one thread, panics if used from another
    // UriSet(Fragile<UriSet<Uri>>), // deprecated
    /// can be used from multiple threads
    Localizer(Arc<UriSetLocalizer>),
}

impl UriSetOrLocalizer {
    // pub fn new_single_thread(uris: UriSet<Uri>) -> Self {
    //     Self::UriSet(Fragile::new(uris))
    // }

    pub fn new_multi_thread(uris: impl Into<HashSet<Uri>>) -> Self {
        Self::Localizer(Arc::new(UriSetLocalizer::new(uris.into())))
    }

    pub fn get(&self) -> UriSet<Uri> {
        match self {
            // UriSetOrLocalizer::UriSet(fragile_uriset) => {
            //     fragile_uriset.try_get().expect("Single-threaded
            // UriSetOrLocalizer called get() on different thread").clone()
            // }
            UriSetOrLocalizer::Localizer(localizer) => localizer.get(),
        }
    }

    pub fn inner_set(&self) -> &HashSet<Uri> {
        match self {
            // UriSetOrLocalizer::UriSet(fragile_uriset) =>
            // fragile_uriset.get().uris.as_ref().clone(),
            UriSetOrLocalizer::Localizer(localizer) => localizer.inner_set(),
        }
    }
}
