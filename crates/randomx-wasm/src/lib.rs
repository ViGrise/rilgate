//! Minimal raw wasm32 ABI around `randomx-core`.
//!
//! The gateway serves this module to browsers, so the surface is intentionally
//! small and free of `wasm-bindgen` glue: the caller owns linear memory and
//! passes pointers into it.

use std::cell::RefCell;

use argon2::Block;
use randomx_core::{
  RandomX,
  cache::RandomXCache,
  constants::{ARGON_MEMORY, CACHE_SIZE},
};

/// Status returned by every fallible export.
const RX_OK: i32 = 0;
const RX_ERROR: i32 = -1;

/// Largest input the ABI accepts, covering a Monero block hashing blob.
const RX_MAX_INPUT: usize = 256;

struct Instance {
  hasher: RandomX,
  #[cfg(target_arch = "wasm32")]
  dataset_module: Vec<u8>,
}

struct Registry {
  instances: Vec<Option<Instance>>,
  imported: Option<Vec<Block>>,
}

thread_local! {
  static REGISTRY: RefCell<Registry> =
    const { RefCell::new(Registry { instances: Vec::new(), imported: None }) };
}

/// Creates a Light mode hasher from `seed_len` bytes at `seed_ptr`.
///
/// Returns a non-negative handle, or [`RX_ERROR`] when the seed cannot
/// initialize an Argon2d cache. Cache construction takes seconds and hundreds
/// of megabytes, so browsers should call this once per seed.
///
/// # Safety
///
/// `seed_ptr` must point at `seed_len` readable bytes inside this module's
/// linear memory for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rx_create(seed_ptr: *const u8, seed_len: usize) -> i32 {
  if seed_len == 0 || seed_len > RX_MAX_INPUT {
    return RX_ERROR;
  }

  let seed = unsafe { std::slice::from_raw_parts(seed_ptr, seed_len) };
  let Ok(instance) = RandomX::new(seed) else {
    return RX_ERROR;
  };

  register(instance)
}

fn register(instance: RandomX) -> i32 {
  let instance = Instance {
    #[cfg(target_arch = "wasm32")]
    dataset_module: instance.cache().dataset_module(),
    hasher: instance,
  };
  REGISTRY.with(|registry| {
    let mut registry = registry.borrow_mut();

    if let Some(index) = registry.instances.iter().position(Option::is_none) {
      registry.instances[index] = Some(instance);
      return index as i32;
    }

    registry.instances.push(Some(instance));
    (registry.instances.len() - 1) as i32
  })
}

/// Returns the required raw cache length.
#[unsafe(no_mangle)]
pub extern "C" fn rx_cache_size() -> usize {
  CACHE_SIZE
}

/// Allocates the raw cache import buffer in module memory.
#[unsafe(no_mangle)]
pub extern "C" fn rx_cache_import_ptr() -> usize {
  REGISTRY.with(|registry| {
    let mut registry = registry.borrow_mut();
    let memory = registry
      .imported
      .get_or_insert_with(|| vec![Block::new(); ARGON_MEMORY as usize]);
    memory.as_mut_ptr() as usize
  })
}

/// Creates a hasher from the filled import buffer and seed.
///
/// # Safety
/// The seed pointer must reference seed_len readable bytes for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rx_create_from_cache(
  seed_ptr: *const u8,
  seed_len: usize,
  cache_len: usize,
) -> i32 {
  if seed_len == 0 || seed_len > RX_MAX_INPUT || cache_len != CACHE_SIZE {
    return RX_ERROR;
  }
  let memory = REGISTRY.with(|registry| registry.borrow_mut().imported.take());
  let Some(memory) = memory else {
    return RX_ERROR;
  };
  let seed = unsafe { std::slice::from_raw_parts(seed_ptr, seed_len) };
  register(RandomX::from_cache(RandomXCache::from_memory(seed, memory)))
}

/// Returns the address of the handle's raw cache bytes.
#[unsafe(no_mangle)]
pub extern "C" fn rx_cache_ptr(handle: i32) -> usize {
  REGISTRY.with(|registry| {
    registry
      .borrow()
      .instances
      .get(handle as usize)
      .and_then(Option::as_ref)
      .map_or(0, |instance| {
        instance.hasher.cache().bytes().as_ptr() as usize
      })
  })
}

/// Returns the length of a valid handle's raw cache.
#[unsafe(no_mangle)]
pub extern "C" fn rx_cache_len(handle: i32) -> usize {
  REGISTRY.with(|registry| {
    registry
      .borrow()
      .instances
      .get(handle as usize)
      .and_then(Option::as_ref)
      .map_or(0, |instance| instance.hasher.cache().bytes().len())
  })
}

/// Releases the cache and scratchpad held by `handle`.
#[unsafe(no_mangle)]
pub extern "C" fn rx_destroy(handle: i32) -> i32 {
  if handle < 0 {
    return RX_ERROR;
  }

  REGISTRY.with(|registry| {
    let mut registry = registry.borrow_mut();

    match registry.instances.get_mut(handle as usize) {
      Some(slot) => {
        *slot = None;
        RX_OK
      }
      None => RX_ERROR,
    }
  })
}

/// Hashes `input_ptr[0..input_len]` into the 32 bytes at `out_ptr`.
///
/// # Safety
///
/// `input_ptr` must point at `input_len` readable bytes and `out_ptr` at 32
/// writable bytes, both inside this module's linear memory, and both must stay
/// valid for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rx_hash(
  handle: i32,
  input_ptr: *const u8,
  input_len: usize,
  out_ptr: *mut u8,
) -> i32 {
  if handle < 0 || input_len == 0 || input_len > RX_MAX_INPUT {
    return RX_ERROR;
  }

  let input = unsafe { std::slice::from_raw_parts(input_ptr, input_len) };
  let output = unsafe { std::slice::from_raw_parts_mut(out_ptr, 32) };

  REGISTRY.with(|registry| {
    let mut registry = registry.borrow_mut();

    let instance = match registry.instances.get_mut(handle as usize) {
      Some(Some(instance)) => instance,
      _ => return RX_ERROR,
    };

    match instance.hasher.hash(input) {
      Ok(hash) => {
        output.copy_from_slice(&hash);
        RX_OK
      }
      Err(_) => RX_ERROR,
    }
  })
}

/// Returns the address of a seed-specialized dataset module owned by the handle.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn rx_dataset_module_ptr(handle: i32) -> usize {
  REGISTRY.with(|registry| {
    registry
      .borrow()
      .instances
      .get(handle as usize)
      .and_then(Option::as_ref)
      .map_or(0, |instance| instance.dataset_module.as_ptr() as usize)
  })
}

#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn rx_dataset_module_len(handle: i32) -> usize {
  REGISTRY.with(|registry| {
    registry
      .borrow()
      .instances
      .get(handle as usize)
      .and_then(Option::as_ref)
      .map_or(0, |instance| instance.dataset_module.len())
  })
}

/// Installs a WASM table entry for the handle's dataset kernel; zero selects interpretation.
///
/// # Safety
/// A nonzero entry must reference the dataset function generated for this handle,
/// instantiated with this module's memory, and remain valid until disabled or destroyed.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rx_set_dataset_function(handle: i32, index: usize) -> i32 {
  REGISTRY.with(|registry| {
    let mut registry = registry.borrow_mut();
    let Some(Some(instance)) = registry.instances.get_mut(handle as usize) else {
      return RX_ERROR;
    };
    let function = if index == 0 {
      None
    } else {
      Some(unsafe { std::mem::transmute::<usize, unsafe extern "C" fn(u64, *mut u64)>(index) })
    };
    unsafe {
      instance.hasher.cache_mut().set_dataset_function(function);
    }
    RX_OK
  })
}

/// Computes an interpreted dataset item into a caller-owned output buffer.
///
/// # Safety
/// The output must be eight-byte aligned and point to 64 writable bytes in linear memory.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rx_dataset_item(handle: i32, item: u64, output: *mut u64) -> i32 {
  REGISTRY.with(|registry| {
    let registry = registry.borrow();
    let Some(Some(instance)) = registry.instances.get(handle as usize) else {
      return RX_ERROR;
    };
    let result = instance.hasher.cache().dataset_item_interpreted(item);
    unsafe {
      std::ptr::copy_nonoverlapping(result.as_ptr(), output, 8);
    }
    RX_OK
  })
}

/// Compares accelerated arithmetic with the software oracle.
#[cfg(feature = "validation")]
#[unsafe(no_mangle)]
pub extern "C" fn rx_validate_float(seed: u64, count: u32) -> i32 {
  if count > 1_000_000 {
    return RX_ERROR;
  }
  if randomx_core::float::validate_arithmetic(seed, count) {
    RX_OK
  } else {
    RX_ERROR
  }
}

/// Installs a WASM table entry for paired dataset computation; zero disables it.
///
/// # Safety
/// A nonzero entry must be this handle's paired dataset function, instantiated
/// with this module's memory, and remain valid until disabled or destroyed.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rx_set_dataset_pair_function(handle: i32, index: usize) -> i32 {
  REGISTRY.with(|registry| {
    let mut registry = registry.borrow_mut();
    let Some(Some(instance)) = registry.instances.get_mut(handle as usize) else {
      return RX_ERROR;
    };
    let function = if index == 0 {
      None
    } else {
      Some(unsafe {
        std::mem::transmute::<usize, unsafe extern "C" fn(u64, u64, *mut u64, *mut u64)>(index)
      })
    };
    unsafe {
      instance
        .hasher
        .cache_mut()
        .set_dataset_pair_function(function);
    }
    RX_OK
  })
}

/// Hashes two equally sized independent inputs into consecutive 32-byte outputs.
///
/// # Safety
/// Both input pointers must reference input_len readable bytes and output must
/// reference 64 writable bytes, all valid for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rx_hash_pair(
  handle: i32,
  first: *const u8,
  second: *const u8,
  input_len: usize,
  output: *mut u8,
) -> i32 {
  if handle < 0 || input_len == 0 || input_len > RX_MAX_INPUT {
    return RX_ERROR;
  }
  let inputs = unsafe {
    [
      std::slice::from_raw_parts(first, input_len),
      std::slice::from_raw_parts(second, input_len),
    ]
  };
  REGISTRY.with(|registry| {
    let mut registry = registry.borrow_mut();
    let Some(Some(instance)) = registry.instances.get_mut(handle as usize) else {
      return RX_ERROR;
    };
    let Ok(result) = instance.hasher.hash_pair(inputs) else {
      return RX_ERROR;
    };
    unsafe {
      std::ptr::copy_nonoverlapping(result[0].as_ptr(), output, 32);
      std::ptr::copy_nonoverlapping(result[1].as_ptr(), output.add(32), 32);
    }
    RX_OK
  })
}

/// Installs a WASM table entry for three-item dataset computation; zero disables it.
///
/// # Safety
/// A nonzero entry must be this handle's three-item dataset function, instantiated
/// with this module's memory, and remain valid until disabled or destroyed.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rx_set_dataset_triple_function(handle: i32, index: usize) -> i32 {
  REGISTRY.with(|registry| {
    let mut registry = registry.borrow_mut();
    let Some(Some(instance)) = registry.instances.get_mut(handle as usize) else {
      return RX_ERROR;
    };
    let function = if index == 0 {
      None
    } else {
      Some(unsafe {
        std::mem::transmute::<usize, unsafe extern "C" fn(*const u64, *mut u64)>(index)
      })
    };
    unsafe {
      instance
        .hasher
        .cache_mut()
        .set_dataset_triple_function(function);
    }
    RX_OK
  })
}

/// Hashes three equally sized inputs spaced 256 bytes apart into consecutive outputs.
///
/// # Safety
/// Each input at input, input + 256 and input + 512 must reference input_len
/// readable bytes. Output must reference 96 writable bytes for the call's duration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rx_hash_triple(
  handle: i32,
  input: *const u8,
  input_len: usize,
  output: *mut u8,
) -> i32 {
  if handle < 0 || input_len == 0 || input_len > RX_MAX_INPUT {
    return RX_ERROR;
  }
  let inputs =
    unsafe { core::array::from_fn(|i| std::slice::from_raw_parts(input.add(i * 256), input_len)) };
  REGISTRY.with(|registry| {
    let mut registry = registry.borrow_mut();
    let Some(Some(instance)) = registry.instances.get_mut(handle as usize) else {
      return RX_ERROR;
    };
    let Ok(result) = instance.hasher.hash_triple(inputs) else {
      return RX_ERROR;
    };
    for (i, hash) in result.iter().enumerate() {
      unsafe {
        std::ptr::copy_nonoverlapping(hash.as_ptr(), output.add(i * 32), 32);
      }
    }
    RX_OK
  })
}
