//! Byte buffers that are statically guaranteed to contain at least one byte.
//!
//! This crate provides two thin wrappers around the buffer types from the [`bytes`] crate.
//! They behave like the types they wrap, except that an empty buffer cannot be represented:
//! if you are holding one, it has a first byte.
//!
//! | Type                 | Wraps               | Mutable | Cloning                       |
//! |----------------------|---------------------|---------|-------------------------------|
//! | [`NonEmptyBytes`]    | [`bytes::Bytes`]    | no      | `O(1)`, shares the storage    |
//! | [`NonEmptyBytesMut`] | [`bytes::BytesMut`] | yes     | copies the contents           |
//!
//! # Why
//!
//! Many protocols and formats treat "zero bytes" as invalid: an empty frame, an empty key, a
//! payload that must carry at least a tag byte. Checking for emptiness at every use site is
//! easy to forget and impossible for the compiler to verify. These types move the check to the
//! point of construction, and everything downstream gets to rely on it:
//!
//! * `len()` returns a [`NonZeroUsize`](core::num::NonZeroUsize), so there is no `is_empty` to
//!   check and no division-by-length or `len() - 1` edge case to guard.
//! * [`NonEmptyBytesMut::first_byte`] and [`NonEmptyBytesMut::last_byte`] are infallible.
//! * Function signatures can say "I need at least one byte" and drop their own checks.
//!
//! # Creating a buffer
//!
//! Construction is fallible, and empty input is rejected. The inherent constructors
//! (`new`, `copy_from_slice`, `from_static`, `from_owner`) return an [`Option`], while the
//! [`TryFrom`] implementations return [`EmptyBytesError`]. Owned inputs such as `Vec<u8>` and
//! `String` are adopted without copying where the underlying `bytes` type allows it.
//!
//! If you can prove the input is non-empty by other means, the `unsafe` `new_unchecked`
//! constructors skip the check. Passing an empty buffer to them is undefined behavior.
//!
//! # How the guarantee is kept
//!
//! The wrappers only expose operations that cannot leave the buffer empty:
//!
//! * Operations that could empty a buffer either take a
//!   [`NonZeroUsize`](core::num::NonZeroUsize) (`truncate`, `resize`, and the split points of
//!   `split_off` and `split_to`) or return an [`Option`] (`slice`, `split_off`, `split_to`).
//! * Operations that only grow a buffer are unrestricted (`extend_from_slice`, `reserve`, ...).
//! * Both types dereference to `[u8]`, so the full read-only slice API is available.
//!   [`NonEmptyBytesMut`] also implements [`DerefMut`](core::ops::DerefMut), which lets you edit
//!   bytes in place but never change the length.
//! * There is no `clear`, and neither type implements [`bytes::Buf`] or [`bytes::BufMut`],
//!   because `advance` and friends would break the guarantee.
//!
//! # Moving between types
//!
//! ```text
//!                          freeze (free)
//!    *------------------* ────────────────▶ *---------------*
//!    | NonEmptyBytesMut |                   | NonEmptyBytes |
//!    *------------------* ◀──────────────── *---------------*
//!    try_into_mut (free if unique) / From (copies if shared)
//! ```
//!
//! * [`NonEmptyBytesMut::freeze`] (or `From`) turns a mutable buffer into an immutable,
//!   cheaply cloneable one at no cost.
//! * [`NonEmptyBytes::try_into_mut`] succeeds without copying only if the handle is the unique
//!   owner of the whole buffer, and otherwise hands the buffer back. The `From` conversion
//!   always succeeds but copies when it cannot take over the allocation, which is always the
//!   case for buffers made with `from_static` or `from_owner`.
//!
//! # Escape hatches
//!
//! `as_inner` gives read-only access to the wrapped [`bytes`] value, which is useful for
//! methods the wrapper does not re-export. `into_inner` (and the `From` conversions into
//! [`Bytes`](bytes::Bytes), [`BytesMut`](bytes::BytesMut) and `Vec<u8>`) give up the guarantee:
//! the result can be emptied freely. To get it back, pass the value to `new` or `try_from`
//! again.
//!
//! # Comparisons
//!
//! Equality and ordering are bytewise. Both types can be compared directly, in either order,
//! with `[u8]`, `&[u8]`, `str`, `&str`, `Vec<u8>`, `String` and [`Bytes`](bytes::Bytes);
//! [`NonEmptyBytesMut`] can additionally be compared with [`BytesMut`](bytes::BytesMut).
//!
//! # Example
//!
//! ```
//! use std::num::NonZeroUsize;
//! use non_empty_bytes::{EmptyBytesError, NonEmptyBytes, NonEmptyBytesMut};
//!
//! // Empty input is rejected up front.
//! assert_eq!(NonEmptyBytes::try_from(Vec::<u8>::new()), Err(EmptyBytesError));
//!
//! // Build a payload in a mutable buffer...
//! let mut buf = NonEmptyBytesMut::try_from("HTTP/1.1")?;
//! buf.extend_from_slice(b" 200 OK");
//!
//! // ...then freeze it into a cheaply cloneable, immutable handle.
//! let bytes: NonEmptyBytes = buf.freeze();
//! let copy = bytes.clone();
//! assert_eq!(&copy[..], b"HTTP/1.1 200 OK");
//!
//! // The length can never be zero, so it is a `NonZeroUsize`.
//! assert_eq!(bytes.len(), NonZeroUsize::new(15).unwrap());
//! # Ok::<(), EmptyBytesError>(())
//! ```

#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std; // tests can use std

#[cfg(feature = "serde")]
mod serde_impl;

use core::fmt;

mod non_empty_bytes;
mod non_empty_bytes_mut;

pub use non_empty_bytes::NonEmptyBytes;
pub use non_empty_bytes_mut::NonEmptyBytesMut;

/// The error returned when a [`NonEmptyBytes`] or [`NonEmptyBytesMut`] is requested from
/// input that contains no bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EmptyBytesError;

impl fmt::Display for EmptyBytesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected at least one byte, got none")
    }
}

impl core::error::Error for EmptyBytesError {}
