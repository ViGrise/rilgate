//! Raw browser ABI with u32 halves for every 64-bit value.

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;
use alloc::{boxed::Box, vec::Vec};
use core::cell::RefCell;

#[cfg(target_arch = "wasm32")]
#[global_allocator]
static ALLOCATOR: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
  core::arch::wasm32::unreachable()
}

use hashwx_core::HashWx;
use sha2::{Digest, Sha256};

struct Instance {
  hasher: HashWx,
  module: Vec<u8>,
}

type InstanceTable = RefCell<Vec<Option<Box<Instance>>>>;

#[cfg(not(target_arch = "wasm32"))]
thread_local! {
  static INSTANCES: InstanceTable = const { RefCell::new(Vec::new()) };
}

#[cfg(target_arch = "wasm32")]
struct InstanceStorage(core::cell::UnsafeCell<InstanceTable>);

// WASM instances have unshared memory and synchronous exports with no imports.
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for InstanceStorage {}

#[cfg(target_arch = "wasm32")]
static INSTANCES: InstanceStorage =
  InstanceStorage(core::cell::UnsafeCell::new(RefCell::new(Vec::new())));

fn with_instances<R>(access: impl FnOnce(&InstanceTable) -> R) -> R {
  #[cfg(not(target_arch = "wasm32"))]
  {
    INSTANCES.with(access)
  }
  #[cfg(target_arch = "wasm32")]
  {
    access(unsafe { &*INSTANCES.0.get() })
  }
}

fn with_instance(handle: i32, f: impl FnOnce(&mut Instance) -> i32) -> i32 {
  with_instances(|instances| {
    let mut instances = instances.borrow_mut();
    match instances.get_mut(handle as usize).and_then(Option::as_mut) {
      Some(instance) => f(instance),
      None => -1,
    }
  })
}

fn join(lo: u32, hi: u32) -> u64 {
  u64::from(lo) | (u64::from(hi) << 32)
}

/// Allocates a zeroed 32-byte caller buffer; memory views must be refreshed afterward.
#[unsafe(no_mangle)]
pub extern "C" fn hx_alloc() -> *mut u8 {
  Box::into_raw(Box::new([0u8; 32])).cast()
}

/// Releases a caller buffer.
///
/// # Safety
/// `pointer` must be an allocation returned by hx_alloc and freed exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hx_free(pointer: *mut u8) {
  unsafe { drop(Box::from_raw(pointer.cast::<[u8; 32]>())) };
}

/// Creates an instance from exactly 32 seed bytes, returning a handle or -1.
///
/// # Safety
/// `seed` must reference 32 readable bytes for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hx_create(seed: *const u8, len: u32) -> i32 {
  if len != 32 {
    return -1;
  }
  let seed = unsafe { &*seed.cast::<[u8; 32]>() };
  let hasher = HashWx::new(seed);
  let module = Vec::new();
  let instance = Box::new(Instance { hasher, module });
  with_instances(|instances| {
    let mut instances = instances.borrow_mut();
    if let Some(index) = instances.iter().position(Option::is_none) {
      instances[index] = Some(instance);
      index as i32
    } else {
      instances.push(Some(instance));
      (instances.len() - 1) as i32
    }
  })
}

/// Destroys a live instance, returning 0 or -1 for an invalid handle.
#[unsafe(no_mangle)]
pub extern "C" fn hx_destroy(handle: i32) -> i32 {
  with_instances(
    |instances| match instances.borrow_mut().get_mut(handle as usize) {
      Some(slot) if slot.is_some() => {
        *slot = None;
        0
      }
      _ => -1,
    },
  )
}

/// Writes the interpreted hash as eight little-endian bytes; returns 0 or -1.
///
/// # Safety
/// `output` must reference eight writable bytes, valid for the call's duration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hx_hash(handle: i32, lo: u32, hi: u32, output: *mut u8) -> i32 {
  with_instance(handle, |instance| {
    let hash = instance.hasher.hash(join(lo, hi));
    unsafe { output.copy_from_nonoverlapping(hash.to_le_bytes().as_ptr(), 8) };
    0
  })
}

/// Searches wrapping consecutive nonces for hash < target; returns 0 or -1.
///
/// # Safety
/// `output` must reference 16 writable bytes. A match writes the nonce followed
/// by the hash in little-endian order. Failure leaves the output unchanged.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hx_search(
  handle: i32,
  lo: u32,
  hi: u32,
  count: u32,
  target_lo: u32,
  target_hi: u32,
  output: *mut u8,
) -> i32 {
  with_instance(handle, |instance| {
    let start = join(lo, hi);
    let target = join(target_lo, target_hi);
    for offset in 0..count {
      let nonce = start.wrapping_add(u64::from(offset));
      let hash = instance.hasher.hash(nonce);
      if hash < target {
        unsafe {
          output.copy_from_nonoverlapping(nonce.to_le_bytes().as_ptr(), 8);
          output
            .add(8)
            .copy_from_nonoverlapping(hash.to_le_bytes().as_ptr(), 8);
        }
        return 0;
      }
    }
    -1
  })
}

/// Returns a module byte pointer, valid until the handle is destroyed; zero on error.
#[unsafe(no_mangle)]
pub extern "C" fn hx_module_ptr(handle: i32) -> usize {
  let mut pointer = 0;
  with_instance(handle, |instance| {
    if instance.module.is_empty() {
      instance.module = instance.hasher.wasm_module();
    }
    pointer = instance.module.as_ptr() as usize;
    0
  });
  pointer
}

/// Returns the generated module length, or zero for an invalid handle.
#[unsafe(no_mangle)]
pub extern "C" fn hx_module_len(handle: i32) -> usize {
  let mut len = 0;
  with_instance(handle, |instance| {
    if instance.module.is_empty() {
      instance.module = instance.hasher.wasm_module();
    }
    len = instance.module.len();
    0
  });
  len
}

/// Derives the seed from a challenge and a little-endian batch number.
///
/// # Safety
/// `challenge` must reference 32 readable bytes and `output` 32 writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn hx_seed(
  challenge: *const u8,
  batch_lo: u32,
  batch_hi: u32,
  output: *mut u8,
) {
  let challenge = unsafe { core::slice::from_raw_parts(challenge, 32) };
  let mut hash = Sha256::new();
  hash.update(challenge);
  hash.update(join(batch_lo, batch_hi).to_le_bytes());
  let seed = hash.finalize();
  unsafe { output.copy_from_nonoverlapping(seed.as_ptr(), 32) };
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn interpreted_search_defers_module_generation_until_requested() {
    let seed = [0u8; 32];
    let handle = unsafe { hx_create(seed.as_ptr(), 32) };
    let mut output = [0; 16];
    unsafe { hx_search(handle, 0, 0, 1, u32::MAX, u32::MAX, output.as_mut_ptr()) };
    with_instance(handle, |instance| {
      assert!(
        instance.module.is_empty(),
        "interpreted work must defer code generation"
      );
      0
    });
    assert!(hx_module_len(handle) > 0);
    let pointer = hx_module_ptr(handle);
    assert_ne!(pointer, 0);
    assert_eq!(hx_module_ptr(handle), pointer);
    assert_eq!(hx_destroy(handle), 0);
  }
}
