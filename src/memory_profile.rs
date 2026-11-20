//! Optional process-wide Rust allocation accounting for the Blotworx profiling
//! build. Enable with `--features blotworx-memory-profile`; the normal build
//! does not install this allocator or pay its atomic accounting cost.

#[cfg(feature = "blotworx-memory-profile")]
use std::alloc::{GlobalAlloc, Layout, System};

#[cfg(feature = "blotworx-memory-profile")]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(feature = "blotworx-memory-profile")]
static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "blotworx-memory-profile")]
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "blotworx-memory-profile")]
pub struct TrackingAllocator;

#[cfg(feature = "blotworx-memory-profile")]
fn record_allocation(bytes: usize) {
    let live = LIVE_BYTES.fetch_add(bytes, Ordering::Relaxed) + bytes;
    let mut peak = PEAK_BYTES.load(Ordering::Relaxed);
    while live > peak {
        match PEAK_BYTES.compare_exchange_weak(peak, live, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(actual) => peak = actual,
        }
    }
}

#[cfg(feature = "blotworx-memory-profile")]
fn record_release(bytes: usize) {
    LIVE_BYTES.fetch_sub(bytes, Ordering::Relaxed);
}

#[cfg(feature = "blotworx-memory-profile")]
unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record_allocation(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record_allocation(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        record_release(layout.size());
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size == 0 {
            unsafe { System.dealloc(pointer, layout) };
            record_release(layout.size());
            return std::ptr::null_mut();
        }
        let result = unsafe { System.realloc(pointer, layout, new_size) };
        if !result.is_null() {
            if new_size > layout.size() {
                record_allocation(new_size - layout.size());
            } else {
                record_release(layout.size() - new_size);
            }
        }
        result
    }
}

/// `(live requested bytes, high-water requested bytes)`. `None` means the
/// current binary was built without the opt-in profiling allocator.
pub fn snapshot() -> Option<(usize, usize)> {
    #[cfg(feature = "blotworx-memory-profile")]
    {
        Some((
            LIVE_BYTES.load(Ordering::Relaxed),
            PEAK_BYTES.load(Ordering::Relaxed),
        ))
    }
    #[cfg(not(feature = "blotworx-memory-profile"))]
    {
        None
    }
}
