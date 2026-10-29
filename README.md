# non-empty-bytes

[![Crates.io](https://img.shields.io/crates/v/non-empty-bytes.svg)](https://crates.io/crates/non-empty-bytes)
[![Docs.rs](https://img.shields.io/docsrs/non-empty-bytes)](https://docs.rs/non-empty-bytes)
[![Build status](https://github.com/Rtificer/non-empty-bytes/actions/workflows/ci.yml/badge.svg)](https://github.com/Rtificer/non-empty-bytes/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/non-empty-bytes.svg)](#license)
[![Downloads](https://img.shields.io/crates/d/non-empty-bytes.svg)](https://crates.io/crates/non-empty-bytes)

Byte buffers that are statically guaranteed to contain at least one byte.

`non-empty-bytes` provides two thin wrappers around the buffer types from the [`bytes`](https://docs.rs/bytes) crate. They behave like the types they wrap, except that an empty buffer cannot be represented: if you are holding one, it has a first byte.

| Type               | Wraps             | Mutable | Cloning                    |
| ------------------ | ----------------- | ------- | -------------------------- |
| `NonEmptyBytes`    | `bytes::Bytes`    | no      | `O(1)`, shares the storage |
| `NonEmptyBytesMut` | `bytes::BytesMut` | yes     | copies the contents        |

## Why

Many protocols and formats treat "zero bytes" as invalid: an empty frame, an empty key, a payload that must carry at least a tag byte. Checking for emptiness at every use site is easy to forget and impossible for the compiler to verify. These types move the check to the point of construction, and everything downstream gets to rely on it:

- `len()` returns a `NonZeroUsize`, so there is no `is_empty` to check and no division-by-length or `len() - 1` edge case to guard.
- `NonEmptyBytesMut::first_byte` and `last_byte` are infallible.
- Function signatures can say "I need at least one byte" and drop their own checks.

## Quick start

```rust
use std::num::NonZeroUsize;
use non_empty_bytes::{EmptyBytesError, NonEmptyBytes, NonEmptyBytesMut};

// Empty input is rejected up front.
assert_eq!(NonEmptyBytes::try_from(Vec::<u8>::new()), Err(EmptyBytesError));

// Build a payload in a mutable buffer...
let mut buf = NonEmptyBytesMut::try_from("HTTP/1.1")?;
buf.extend_from_slice(b" 200 OK");

// ...then freeze it into a cheaply cloneable, immutable handle.
let bytes: NonEmptyBytes = buf.freeze();
let copy = bytes.clone();
assert_eq!(&copy[..], b"HTTP/1.1 200 OK");

// The length can never be zero, so it is a `NonZeroUsize`.
assert_eq!(bytes.len(), NonZeroUsize::new(15).unwrap());
# Ok::<(), EmptyBytesError>(())
```

## Creating a buffer

Construction is fallible, and empty input is rejected.

- The inherent constructors (`new`, `copy_from_slice`, `from_static`, `from_owner`) return an `Option`.
- The `TryFrom` implementations return `EmptyBytesError`. `NonEmptyBytes` accepts `Bytes`, `&'static [u8]`, `&'static str`, `Vec<u8>`, `Box<[u8]>` and `String`; `NonEmptyBytesMut` accepts `BytesMut`, `&[u8]`, `&str`, `Bytes`, `Vec<u8>`, `Box<[u8]>` and `String`.
- `NonEmptyBytesMut::zeroed` takes a `NonZeroUsize` and cannot fail.
  Owned inputs such as `Vec<u8>` and `String` are adopted without copying where the underlying `bytes` type allows it.

If you can prove the input is non-empty by other means, the `unsafe` `new_unchecked` constructors skip the check. **Passing an empty buffer to them is undefined behavior.**

## How the guarantee is kept

The wrappers only expose operations that cannot leave the buffer empty:

- Operations that could empty a buffer either take a `NonZeroUsize` (`truncate`, `resize`, and the split points of `split_off` and `split_to`) or return an `Option` (`slice`, `split_off`, `split_to`).
- Operations that only grow a buffer are unrestricted (`extend_from_slice`, `reserve`, ...).
- Both types dereference to `[u8]`, so the full read-only slice API is available. `NonEmptyBytesMut` also implements `DerefMut`, which lets you edit bytes in place but never change the length.
- There is no `clear`, and neither type implements `bytes::Buf` or `bytes::BufMut`, because `advance` and friends would break the guarantee.

## Moving between types

```text
                         freeze (free)
   *------------------* ────────────────▶ *---------------*
   | NonEmptyBytesMut |                   | NonEmptyBytes |
   *------------------* ◀──────────────── *---------------*
   try_into_mut (free if unique) / From (copies if shared)
```

- `NonEmptyBytesMut::freeze` (or `From`) turns a mutable buffer into an immutable, cheaply cloneable one at no cost.
- `NonEmptyBytes::try_into_mut` succeeds without copying only if the handle is the unique owner of the whole buffer, and otherwise hands the buffer back. The `From` conversion always succeeds but copies when it cannot take over the allocation, which is always the case for buffers made with `from_static` or `from_owner`.

## Escape hatches

`as_inner` gives read-only access to the wrapped `bytes` value, which is useful for methods the wrapper does not re-export.

`into_inner` (and the `From` conversions into `Bytes`, `BytesMut` and `Vec<u8>`) give up the guarantee: the result can be emptied freely. To get it back, pass the value to `new` or `try_from` again.

## Comparisons

Equality and ordering are bytewise. Both types can be compared directly, in either order, with `[u8]`, `&[u8]`, `str`, `&str`, `Vec<u8>`, `String` and `Bytes`. `NonEmptyBytesMut` can additionally be compared with `BytesMut`.

## Feature flags

| Feature           | Default | Description                                                                                  |
| ----------------- | ------- | -------------------------------------------------------------------------------------------- |
| `std`             | yes     | Enables `std` support in `bytes` (and in `serde`, if enabled).                               |
| `serde`           | no      | Implements `Serialize` and `Deserialize` for both types (see below).                         |
| `extra-platforms` | no      | Passes through `bytes/extra-platforms` for targets without native atomics (e.g. `thumbv6m`). |

### Serde

With the `serde` feature enabled, both types implement `Serialize` and `Deserialize`.

- **Serializing** writes the contents as a byte string (`serialize_bytes`), the same representation `bytes` uses.
- **Deserializing** reads a `Bytes`/`BytesMut` and then validates it. Empty input is rejected with an `invalid_length` error ("expected at least one byte"), so a deserialized value always upholds the non-empty guarantee.

```toml
[dependencies]
non-empty-bytes = { version = "1.0", features = ["serde"] }
```

### `no_std`

The crate is `#![no_std]` and only requires `alloc`. To use it without `std`:

```toml
[dependencies]
non-empty-bytes = { version = "1.0", default-features = false }
```

## License

Licensed under the GPL-3.0 (or any later version). See the repository for details.

## Contributing

Issues and pull requests are welcome at <https://github.com/Rtificer/non-empty-bytes>.
