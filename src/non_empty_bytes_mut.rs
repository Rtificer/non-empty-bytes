//! `NonEmptyBytesMut`: a `BytesMut` that is guaranteed to contain at least one byte.
use crate::EmptyBytesError;
use crate::non_empty_bytes::NonEmptyBytes;
use alloc::{boxed::Box, string::String, vec::Vec};
use bytes::buf::IntoIter;
use bytes::{Bytes, BytesMut};
use core::borrow::{Borrow, BorrowMut};
use core::cmp::Ordering;
use core::mem::MaybeUninit;
use core::num::NonZeroUsize;
use core::ops::{Deref, DerefMut, RangeBounds};
use core::{fmt, slice};

/// A uniquely owned, growable, mutable byte buffer that always contains at least one byte.
///
/// [`NonEmptyBytesMut`] wraps a [`BytesMut`] and adds exactly one guarantee:
/// [`len`](Self::len) is never zero. Everything else is inherited from `BytesMut`. Unlike
/// [`NonEmptyBytes`], cloning copies the data. [`split_off`](Self::split_off) and
/// [`split_to`](Self::split_to) are `O(1)`: the two halves share one allocation but cover
/// disjoint ranges, so each can be mutated independently.
///
/// Once the contents are final, [`freeze`](Self::freeze) converts the buffer into a cheaply
/// cloneable, immutable [`NonEmptyBytes`] at no cost.
///
/// # Length versus capacity
///
/// The guarantee is about the *length*, the number of initialized bytes. The
/// [`capacity`](Self::capacity) can be larger, and the spare capacity is not part of the
/// contents. Since the capacity is never less than the length, it is also returned as a
/// [`NonZeroUsize`].
///
/// # How the guarantee is kept
///
/// * **Growing is unrestricted:** [`extend_from_slice`](Self::extend_from_slice),
///   [`extend_from_within`](Self::extend_from_within), [`reserve`](Self::reserve), the
///   [`Extend`] impls and [`fmt::Write`] (so `write!` works).
/// * **Shrinking has to prove the result is non-empty:** [`truncate`](Self::truncate) and
///   [`resize`](Self::resize) take a [`NonZeroUsize`], while [`split_off`](Self::split_off) and
///   [`split_to`](Self::split_to) return an [`Option`]. There is no [`clear`](BytesMut::clear), and the type does
///   not implement [`bytes::Buf`] or [`bytes::BufMut`], since [`bytes::Buf::advance`] and friends would break
///   the guarantee. Use [`into_inner`](Self::into_inner) if you need them.
/// * **In-place edits are always fine:** the type implements [`DerefMut`] to [`[u8]`], along with
///   [`AsMut`] and [`BorrowMut`], which can change byte values but not the length.
///
/// Reading works through [`Deref`], [`AsRef`], [`Borrow`] and [`IntoIterator`], so the whole
/// slice API is available. [`first_byte`](Self::first_byte) and
/// [`last_byte`](Self::last_byte) are infallible shortcuts.
///
/// # Creating a value
///
/// * [`zeroed`](Self::zeroed): `len` zero bytes, for a non-zero `len`.
/// * [`copy_from_slice`](Self::copy_from_slice): copies a borrowed slice.
/// * [`new`](Self::new): from an existing [`BytesMut`].
/// * [`TryFrom`] from [`BytesMut`], `&[u8]`, `&str`, [`Bytes`], [`Vec<u8>`], [`Box<[u8]>`] and
///   `String`, failing with [`EmptyBytesError`] if the input is empty.
/// * [`From<NonEmptyBytes>`]: copies unless the source uniquely owns its buffer. See
///   [`NonEmptyBytes::try_into_mut`] for a variant that never copies.
/// * [`new_unchecked`](Self::new_unchecked): `unsafe`, skips the check.
///
/// The constructors that can fail return [`None`] when the input is empty. There is
/// deliberately no `Default` implementation, since the default value would be empty.
///
/// # Comparisons
///
/// Equality and ordering are bytewise. A [`NonEmptyBytesMut`] can be compared directly, in
/// either order, with [`BytesMut`], [`Bytes`], [`[u8]`], [`&[u8]`], [`str`], [`&str`], [`Vec<u8>`] and
/// [`String`].
///
/// # Examples
///
/// ```
/// use core::num::NonZeroUsize;
/// use non_empty_bytes::NonEmptyBytesMut;
///
/// let mut buf = NonEmptyBytesMut::try_from(&b"hello"[..]).unwrap();
///
/// // Edit in place; the length cannot change this way.
/// buf[0] = b'j';
///
/// // Grow freely...
/// buf.extend_from_slice(b" world");
/// assert_eq!(&buf[..], b"jello world");
///
/// // ...but shrinking requires a non-zero length.
/// buf.truncate(NonZeroUsize::new(5).unwrap());
/// assert_eq!(&buf[..], b"jello");
/// assert_eq!(buf.first_byte(), b'j');
/// assert_eq!(buf.last_byte(), b'o');
///
/// // Freeze into an immutable handle that is cheap to clone and share.
/// let frozen = buf.freeze();
/// let other = frozen.clone();
/// assert_eq!(frozen, other);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NonEmptyBytesMut(BytesMut);

impl NonEmptyBytesMut {
    #[inline]
    #[must_use]
    pub fn new(bytes: BytesMut) -> Option<Self> {
        if bytes.is_empty() {
            None
        } else {
            Some(Self(bytes))
        }
    }

    /// # Safety
    /// `bytes` must not be empty.
    #[inline]
    #[must_use]
    pub unsafe fn new_unchecked(bytes: BytesMut) -> Self {
        debug_assert!(!bytes.is_empty());
        Self(bytes)
    }

    #[inline]
    #[must_use]
    pub fn copy_from_slice(data: &[u8]) -> Option<Self> {
        if data.is_empty() {
            None
        } else {
            Some(Self(BytesMut::from(data)))
        }
    }

    /// Creates a new `NonEmptyBytesMut` containing `len` zeros, where `len` is non-zero.
    ///
    /// The resulting object has a length of `len` and a capacity greater
    /// than or equal to `len`. The entire length of the object will be filled
    /// with zeros.
    ///
    /// On some platforms or allocators this function may be faster than
    /// a manual implementation.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::num::NonZeroUsize;
    /// use non_empty_bytes::NonEmptyBytesMut;
    ///
    /// let zeros = NonEmptyBytesMut::zeroed(NonZeroUsize::new(42).unwrap());
    ///
    /// assert!(zeros.capacity().get() >= 42);
    /// assert_eq!(zeros.len().get(), 42);
    /// zeros.into_iter().for_each(|x| assert_eq!(x, 0));
    /// ```
    #[must_use]
    pub fn zeroed(len: NonZeroUsize) -> Self {
        Self(BytesMut::zeroed(len.get()))
    }

    #[inline]
    #[must_use]
    pub const fn as_inner(&self) -> &BytesMut {
        &self.0
    }

    #[inline]
    #[must_use]
    pub fn into_inner(self) -> BytesMut {
        self.0
    }

    #[inline]
    #[must_use]
    #[allow(clippy::len_without_is_empty)] // never empty
    pub fn len(&self) -> NonZeroUsize {
        unsafe { NonZeroUsize::new_unchecked(self.0.len()) }
    }

    /// Returns the number of bytes the `NonEmptyBytesMut` can hold without reallocating.
    ///
    /// # Examples
    ///
    /// ```
    /// use non_empty_bytes::NonEmptyBytesMut;
    ///
    /// let mut b = NonEmptyBytesMut::try_from(&b"hello"[..]).unwrap();
    /// b.reserve(64);
    /// assert!(b.capacity().get() >= 69);
    /// ```
    #[inline]
    #[must_use]
    pub fn capacity(&self) -> NonZeroUsize {
        unsafe { NonZeroUsize::new_unchecked(self.0.capacity()) }
    }

    #[inline]
    #[must_use]
    pub fn first_byte(&self) -> u8 {
        self.0[0]
    }

    #[inline]
    #[must_use]
    pub fn last_byte(&self) -> u8 {
        self.0[self.0.len() - 1]
    }

    #[inline]
    #[must_use]
    fn raw(&self) -> &[u8] {
        &self.0
    }

    /// Returns the remaining spare capacity of the buffer as a slice of `MaybeUninit<u8>`.
    ///
    /// The returned slice starts right after the buffer's current length and can be
    /// used to fill the buffer with data (e.g. by reading from a file) before marking
    /// the data as initialized using the [`set_len`] method.
    ///
    /// [`set_len`]: NonEmptyBytesMut::set_len
    ///
    /// # Examples
    ///
    /// ```
    /// use core::num::NonZeroUsize;
    /// use non_empty_bytes::NonEmptyBytesMut;
    ///
    /// // Start with one initialized byte and make room for at least 10 more.
    /// let mut buf = NonEmptyBytesMut::try_from(&[9u8][..]).unwrap();
    /// buf.reserve(10);
    ///
    /// // Fill in the first 3 spare elements.
    /// let uninit = buf.spare_capacity_mut();
    /// uninit[0].write(0);
    /// uninit[1].write(1);
    /// uninit[2].write(2);
    ///
    /// // Mark the 3 newly written bytes as initialized (the new length is 1 + 3).
    /// unsafe { buf.set_len(NonZeroUsize::new(4).unwrap()) };
    ///
    /// assert_eq!(&buf[..], &[9, 0, 1, 2]);
    /// ```
    #[inline]
    #[must_use]
    pub fn spare_capacity_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        self.0.spare_capacity_mut()
    }

    /// Converts `self` into an immutable `NonEmptyBytes`.
    ///
    /// The conversion is zero cost and is used to indicate that the slice
    /// referenced by the handle will no longer be mutated. Once the conversion
    /// is done, the handle can be cloned and shared across threads.
    ///
    /// # Examples
    ///
    /// ```ignore-wasm
    /// use non_empty_bytes::NonEmptyBytesMut;
    /// use std::thread;
    ///
    /// let mut b = NonEmptyBytesMut::try_from(&b"hello"[..]).unwrap();
    /// b.extend_from_slice(b" world");
    /// let b1 = b.freeze();
    /// let b2 = b1.clone();
    ///
    /// let th = thread::spawn(move || {
    ///     assert_eq!(&b1[..], b"hello world");
    /// });
    ///
    /// assert_eq!(&b2[..], b"hello world");
    /// th.join().unwrap();
    /// ```
    #[inline]
    #[must_use]
    pub fn freeze(self) -> NonEmptyBytes {
        unsafe { NonEmptyBytes::new_unchecked(self.0.freeze()) }
    }

    /// Shortens the buffer, keeping the first `len` bytes and dropping the
    /// rest.
    ///
    /// `len` is a [`NonZeroUsize`], so the buffer always keeps at least one byte.
    /// If `len` is greater than the buffer's current length, this has no
    /// effect.
    ///
    /// Existing underlying capacity is preserved.
    ///
    /// The [split_off](`Self::split_off()`) method can emulate `truncate`, but this causes the
    /// excess bytes to be returned instead of dropped.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::num::NonZeroUsize;
    /// use non_empty_bytes::NonEmptyBytesMut;
    ///
    /// let mut buf = NonEmptyBytesMut::try_from(&b"hello world"[..]).unwrap();
    /// buf.truncate(NonZeroUsize::new(5).unwrap());
    /// assert_eq!(buf, b"hello"[..]);
    /// ```
    #[inline]
    pub fn truncate(&mut self, len: NonZeroUsize) {
        self.0.truncate(len.get());
    }

    /// Resizes the buffer so that `len` is equal to `new_len`.
    ///
    /// If `new_len` is greater than `len`, the buffer is extended by the
    /// difference with each additional byte set to `value`. If `new_len` is
    /// less than `len`, the buffer is simply truncated. `new_len` is a
    /// [`NonZeroUsize`], so the buffer can never be resized to empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::num::NonZeroUsize;
    /// use non_empty_bytes::NonEmptyBytesMut;
    ///
    /// let mut buf = NonEmptyBytesMut::try_from(&[0x1][..]).unwrap();
    ///
    /// buf.resize(NonZeroUsize::new(3).unwrap(), 0x1);
    /// assert_eq!(&buf[..], &[0x1, 0x1, 0x1]);
    ///
    /// buf.resize(NonZeroUsize::new(2).unwrap(), 0x2);
    /// assert_eq!(&buf[..], &[0x1, 0x1]);
    ///
    /// buf.resize(NonZeroUsize::new(4).unwrap(), 0x3);
    /// assert_eq!(&buf[..], &[0x1, 0x1, 0x3, 0x3]);
    /// ```
    #[inline]
    pub fn resize(&mut self, new_len: NonZeroUsize, value: u8) {
        self.0.resize(new_len.get(), value);
    }

    /// Sets the length of the buffer.
    ///
    /// `len` is a [`NonZeroUsize`], so the buffer can never
    /// be set to empty.
    ///
    /// # Safety
    ///
    /// This function explicitly sets the size of the buffer without actually
    /// modifying the data, so it is up to the caller to ensure that the data
    /// has been initialized.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::num::NonZeroUsize;
    /// use non_empty_bytes::NonEmptyBytesMut;
    ///
    /// let mut b = NonEmptyBytesMut::try_from(&b"hello world"[..]).unwrap();
    ///
    /// unsafe { b.set_len(NonZeroUsize::new(5).unwrap()) };
    ///
    /// assert_eq!(&b[..], b"hello");
    ///
    /// unsafe { b.set_len(NonZeroUsize::new(11).unwrap()) };
    ///
    /// assert_eq!(&b[..], b"hello world");
    /// ```
    #[inline]
    pub unsafe fn set_len(&mut self, len: NonZeroUsize) {
        unsafe { self.0.set_len(len.get()) };
    }

    /// Splits the buffer into two at the given index, or returns [`None`] if
    /// `at == 0` or `at >= self.len()`.
    ///
    /// Afterwards `self` contains elements `[at, len)`, and the returned `NonEmptyBytesMut`
    /// contains elements `[0, at)`. Since `0 < at < len`, both halves are non-empty.
    /// If [`None`] is returned, `self` is left unchanged.
    ///
    /// This is an `O(1)` operation that just increases the reference count and
    /// sets a few indices.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::num::NonZeroUsize;
    /// use non_empty_bytes::NonEmptyBytesMut;
    ///
    /// let mut a = NonEmptyBytesMut::try_from(&b"hello world"[..]).unwrap();
    /// let mut b = a.split_to(NonZeroUsize::new(5).unwrap()).unwrap();
    ///
    /// a[0] = b'!';
    /// b[0] = b'j';
    ///
    /// assert_eq!(&a[..], b"!world");
    /// assert_eq!(&b[..], b"jello");
    ///
    /// // splitting at/past the end would leave one half empty
    /// assert!(a.split_to(NonZeroUsize::new(6).unwrap()).is_none());
    /// ```
    #[must_use = "consider discarding the prefix another way if you don't need it"]
    pub fn split_to(&mut self, at: NonZeroUsize) -> Option<Self> {
        if at >= self.len() {
            return None;
        }
        Some(Self(self.0.split_to(at.get())))
    }

    /// Splits the bytes into two at the given index, or returns [`None`] if
    /// `at == 0` or `at >= self.len()`.
    ///
    /// Afterwards `self` contains elements `[0, at)`, and the returned
    /// `NonEmptyBytesMut` contains elements `[at, len)`, along with any spare capacity
    /// that followed them. Since `0 < at < len`, both halves are non-empty. If [`None`]
    /// is returned, `self` is left unchanged. It's guaranteed that the memory does not
    /// move, that is, the address of `self` does not change, and the address of the
    /// returned slice is `at` bytes after that.
    ///
    /// This is an `O(1)` operation that just increases the reference count
    /// and sets a few indices.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::num::NonZeroUsize;
    /// use non_empty_bytes::NonEmptyBytesMut;
    ///
    /// let mut a = NonEmptyBytesMut::try_from(&b"hello world"[..]).unwrap();
    /// let mut b = a.split_off(NonZeroUsize::new(5).unwrap()).unwrap();
    ///
    /// a[0] = b'j';
    /// b[0] = b'!';
    ///
    /// assert_eq!(&a[..], b"jello");
    /// assert_eq!(&b[..], b"!world");
    ///
    /// // splitting at `0` or at/past the end would leave one half empty
    /// assert!(a.split_off(NonZeroUsize::new(5).unwrap()).is_none());
    /// ```
    #[must_use = "consider truncate if you don't need the other half"]
    pub fn split_off(&mut self, at: NonZeroUsize) -> Option<Self> {
        if at >= self.len() {
            return None;
        }
        Some(Self(self.0.split_off(at.get())))
    }

    /// Absorbs a [`BytesMut`] that was previously split off if they are
    /// contiguous, otherwise appends its bytes to this `NonEmptyBytesMut`.
    ///
    /// If the two buffers were previously contiguous and not mutated
    /// in a way that causes re-allocation i.e., if `other` was created by
    /// calling `split_off` on this `NonEmptyBytesMut` (and converted with
    /// [`into_inner`](Self::into_inner)), then this is an `O(1)` operation
    /// that just decreases a reference count and sets a few indices.
    /// Otherwise this method degenerates to
    /// `self.extend_from_slice(other.as_ref())`.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::num::NonZeroUsize;
    /// use non_empty_bytes::NonEmptyBytesMut;
    ///
    /// let mut buf = NonEmptyBytesMut::try_from(&b"aaabbbcccddd"[..]).unwrap();
    ///
    /// let split = buf.split_off(NonZeroUsize::new(6).unwrap()).unwrap();
    /// assert_eq!(b"aaabbb", &buf[..]);
    /// assert_eq!(b"cccddd", &split[..]);
    ///
    /// buf.unsplit(split.into_inner());
    /// assert_eq!(b"aaabbbcccddd", &buf[..]);
    /// ```
    #[inline]
    pub fn unsplit(&mut self, other: BytesMut) {
        self.0.unsplit(other);
    }

    /// Absorbs a [`BytesMut`] that was previously split off.
    ///
    /// If the two buffers were previously contiguous, i.e., if
    /// `other` was created by calling `split_off` on this `NonEmptyBytesMut` (and
    /// converted with [`into_inner`](Self::into_inner)), then
    /// this is an `O(1)` operation that just decreases a reference
    /// count and sets a few indices. Otherwise this method returns an error
    /// containing the original `other`.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::num::NonZeroUsize;
    /// use non_empty_bytes::NonEmptyBytesMut;
    ///
    /// let mut buf = NonEmptyBytesMut::try_from(&b"aaabbbcccddd"[..]).unwrap();
    ///
    /// let mut split_1 = buf.split_off(NonZeroUsize::new(3).unwrap()).unwrap();
    /// let split_2 = split_1.split_off(NonZeroUsize::new(3).unwrap()).unwrap();
    /// assert_eq!(b"aaa", &buf[..]);
    /// assert_eq!(b"bbb", &split_1[..]);
    /// assert_eq!(b"cccddd", &split_2[..]);
    ///
    /// let split_2 = buf.try_unsplit(split_2.into_inner()).unwrap_err();
    ///
    /// buf.try_unsplit(split_1.into_inner()).unwrap();
    /// buf.try_unsplit(split_2).unwrap();
    /// assert_eq!(b"aaabbbcccddd", &buf[..]);
    /// ```
    #[inline]
    pub fn try_unsplit(&mut self, other: BytesMut) -> Result<(), BytesMut> {
        self.0.try_unsplit(other)
    }

    /// Reserves capacity for at least `additional` more bytes to be inserted
    /// into the given `NonEmptyBytesMut`.
    ///
    /// More than `additional` bytes may be reserved in order to avoid frequent
    /// reallocations. A call to `reserve` may result in an allocation.
    ///
    /// Before allocating new buffer space, the function will attempt to reclaim
    /// space in the existing buffer. If the current handle references a view
    /// into a larger original buffer, and all other handles referencing part
    /// of the same original buffer have been dropped, then the current view
    /// can be copied/shifted to the front of the buffer and the handle can take
    /// ownership of the full buffer, provided that the full buffer is large
    /// enough to fit the requested additional capacity.
    ///
    /// This optimization will only happen if shifting the data from the current
    /// view to the front of the buffer is not too expensive in terms of the
    /// (amortized) time required. The precise condition is subject to change;
    /// as of now, the length of the data being shifted needs to be at least as
    /// large as the distance that it's shifted by.
    ///
    /// This method does not preserve data stored in the unused capacity.
    ///
    /// # Examples
    ///
    /// In the following example, a new buffer is allocated.
    ///
    /// ```
    /// use non_empty_bytes::NonEmptyBytesMut;
    ///
    /// let mut buf = NonEmptyBytesMut::try_from(&b"hello"[..]).unwrap();
    /// buf.reserve(64);
    /// assert!(buf.capacity().get() >= 69);
    /// ```
    ///
    /// In the following example, the existing buffer is reclaimed.
    ///
    /// ```
    /// use core::num::NonZeroUsize;
    /// use non_empty_bytes::NonEmptyBytesMut;
    ///
    /// let mut buf = NonEmptyBytesMut::try_from(&[0u8; 64][..]).unwrap();
    /// buf.reserve(64);
    ///
    /// let capacity = buf.capacity().get();
    /// let ptr = buf.as_ptr();
    ///
    /// // `head` shares the allocation, so `buf` now starts 32 bytes in.
    /// let head = buf.split_to(NonZeroUsize::new(32).unwrap()).unwrap();
    /// assert_eq!(buf.capacity().get(), capacity - 32);
    ///
    /// // Once `head` is dropped, the full allocation is reclaimed instead of
    /// // allocating a new buffer.
    /// drop(head);
    /// buf.reserve(capacity - 32);
    ///
    /// assert_eq!(buf.capacity().get(), capacity);
    /// assert_eq!(buf.as_ptr(), ptr);
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the new capacity overflows `usize`.
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.0.reserve(additional);
    }

    /// Attempts to cheaply reclaim already allocated capacity for at least `additional` more
    /// bytes to be inserted into the given `NonEmptyBytesMut` and returns `true` if it succeeded.
    ///
    /// `try_reclaim` behaves exactly like `reserve`, except that it never allocates new storage
    /// and returns a `bool` indicating whether it was successful in doing so:
    ///
    /// `try_reclaim` returns false under these conditions:
    ///  - The spare capacity left is less than `additional` bytes AND
    ///  - The existing allocation cannot be reclaimed cheaply or it was less than
    ///    `additional` bytes in size
    ///
    /// Reclaiming the allocation cheaply is possible if the `NonEmptyBytesMut` has no outstanding
    /// references through other `NonEmptyBytesMut`s or `NonEmptyBytes` which point to the same underlying
    /// storage.
    ///
    /// This method does not preserve data stored in the unused capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::num::NonZeroUsize;
    /// use non_empty_bytes::NonEmptyBytesMut;
    ///
    /// let mut buf = NonEmptyBytesMut::try_from(&b"abcd"[..]).unwrap();
    /// buf.reserve(64);
    ///
    /// // There is already enough spare capacity, so nothing needs to be allocated.
    /// assert!(buf.try_reclaim(60));
    ///
    /// // The allocation is too small to ever satisfy this request.
    /// assert!(!buf.try_reclaim(1 << 20));
    ///
    /// // `tail` shares the allocation with `buf`, so `buf` cannot reclaim it.
    /// let tail = buf.split_off(NonZeroUsize::new(2).unwrap()).unwrap();
    /// assert!(!buf.try_reclaim(60));
    ///
    /// // With no other handles left, the whole allocation can be reclaimed.
    /// drop(tail);
    /// assert!(buf.try_reclaim(60));
    /// ```
    #[inline]
    #[must_use = "consider NonEmptyBytesMut::reserve if you need an infallible reservation"]
    pub fn try_reclaim(&mut self, additional: usize) -> bool {
        self.0.try_reclaim(additional)
    }

    /// Appends given bytes to this `NonEmptyBytesMut`.
    ///
    /// If this `NonEmptyBytesMut` object does not have enough capacity, it is resized
    /// first. Appending never shrinks the buffer, so appending an empty slice is a no-op.
    ///
    /// # Examples
    ///
    /// ```
    /// use non_empty_bytes::NonEmptyBytesMut;
    ///
    /// let mut buf = NonEmptyBytesMut::try_from(&b"aaabbb"[..]).unwrap();
    /// buf.extend_from_slice(b"cccddd");
    ///
    /// assert_eq!(b"aaabbbcccddd", &buf[..]);
    /// ```
    #[inline]
    pub fn extend_from_slice(&mut self, extend: &[u8]) {
        self.0.extend_from_slice(extend);
    }

    /// Clones the elements in the given `range` within this `NonEmptyBytesMut` and
    /// appends them to the end.
    ///
    /// # Panics
    ///
    /// Panics if `range` is out of bounds for this `NonEmptyBytesMut`.
    ///
    /// # Examples
    ///
    /// ```
    /// use non_empty_bytes::NonEmptyBytesMut;
    ///
    /// let mut buf = NonEmptyBytesMut::try_from(&b"aaabbb_"[..]).unwrap();
    /// buf.extend_from_within(3..6);
    ///
    /// assert_eq!(b"aaabbb_bbb", &buf[..]);
    /// ```
    #[inline]
    pub fn extend_from_within(&mut self, range: impl RangeBounds<usize>) {
        self.0.extend_from_within(range);
    }
}

// ------------------------------------------------------- NonEmptyBytes glue

impl From<NonEmptyBytes> for NonEmptyBytesMut {
    /// Converts a `NonEmptyBytes` into a `NonEmptyBytesMut`.
    ///
    /// If `b` is unique for the entire original buffer, this will return a
    /// `NonEmptyBytesMut` with the contents of `b` without copying.
    /// If `b` is not unique for the entire original buffer, this will make
    /// a copy of `b`'s subset of the original buffer in a new `NonEmptyBytesMut`.
    ///
    /// # Examples
    ///
    /// ```
    /// use non_empty_bytes::{NonEmptyBytes, NonEmptyBytesMut};
    ///
    /// let bytes = NonEmptyBytes::try_from(b"hello".to_vec()).unwrap();
    /// assert_eq!(
    ///     NonEmptyBytesMut::from(bytes),
    ///     NonEmptyBytesMut::try_from(&b"hello"[..]).unwrap()
    /// );
    /// ```
    fn from(b: NonEmptyBytes) -> Self {
        Self(BytesMut::from(b.into_inner()))
    }
}

impl From<NonEmptyBytesMut> for NonEmptyBytes {
    fn from(b: NonEmptyBytesMut) -> Self {
        b.freeze()
    }
}

// ------------------------------------------------------------ std traits

impl AsRef<[u8]> for NonEmptyBytesMut {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl AsMut<[u8]> for NonEmptyBytesMut {
    #[inline]
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

impl Deref for NonEmptyBytesMut {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// Mutating bytes in place through a slice cannot change the length.
impl DerefMut for NonEmptyBytesMut {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Borrow<[u8]> for NonEmptyBytesMut {
    #[inline]
    fn borrow(&self) -> &[u8] {
        &self.0
    }
}

impl BorrowMut<[u8]> for NonEmptyBytesMut {
    #[inline]
    fn borrow_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }
}

impl fmt::Write for NonEmptyBytesMut {
    #[inline]
    fn write_str(&mut self, s: &str) -> fmt::Result {
        fmt::Write::write_str(&mut self.0, s)
    }
}

impl Extend<u8> for NonEmptyBytesMut {
    fn extend<T: IntoIterator<Item = u8>>(&mut self, iter: T) {
        self.0.extend(iter);
    }
}

impl<'a> Extend<&'a u8> for NonEmptyBytesMut {
    fn extend<T: IntoIterator<Item = &'a u8>>(&mut self, iter: T) {
        self.0.extend(iter);
    }
}

impl Extend<Bytes> for NonEmptyBytesMut {
    fn extend<T: IntoIterator<Item = Bytes>>(&mut self, iter: T) {
        self.0.extend(iter);
    }
}

impl IntoIterator for NonEmptyBytesMut {
    type Item = u8;
    type IntoIter = IntoIter<BytesMut>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a NonEmptyBytesMut {
    type Item = &'a u8;
    type IntoIter = slice::Iter<'a, u8>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'a> IntoIterator for &'a mut NonEmptyBytesMut {
    type Item = &'a mut u8;
    type IntoIter = slice::IterMut<'a, u8>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter_mut()
    }
}

// ------------------------------------------------------------ conversions

impl From<NonEmptyBytesMut> for BytesMut {
    fn from(b: NonEmptyBytesMut) -> BytesMut {
        b.0
    }
}

impl From<NonEmptyBytesMut> for Bytes {
    fn from(b: NonEmptyBytesMut) -> Bytes {
        b.0.freeze()
    }
}

impl From<NonEmptyBytesMut> for Vec<u8> {
    fn from(b: NonEmptyBytesMut) -> Vec<u8> {
        b.0.into()
    }
}

impl TryFrom<BytesMut> for NonEmptyBytesMut {
    type Error = EmptyBytesError;

    #[inline]
    fn try_from(value: BytesMut) -> Result<Self, Self::Error> {
        Self::new(value).ok_or(EmptyBytesError)
    }
}

impl TryFrom<&[u8]> for NonEmptyBytesMut {
    type Error = EmptyBytesError;

    #[inline]
    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::copy_from_slice(value).ok_or(EmptyBytesError)
    }
}

impl TryFrom<&str> for NonEmptyBytesMut {
    type Error = EmptyBytesError;

    #[inline]
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::copy_from_slice(value.as_bytes()).ok_or(EmptyBytesError)
    }
}

// Owned inputs go through `Bytes` so the allocation can be reused when unique.
macro_rules! impl_try_from_owned {
    ($($t:ty),* $(,)?) => {$(
        impl TryFrom<$t> for NonEmptyBytesMut {
            type Error = EmptyBytesError;

            #[inline]
            fn try_from(value: $t) -> Result<Self, Self::Error> {
                Self::new(BytesMut::from(Bytes::from(value))).ok_or(EmptyBytesError)
            }
        }
    )*};
}

impl_try_from_owned!(Bytes, Vec<u8>, Box<[u8]>, String);

// ------------------------------------------------------------- comparisons

macro_rules! impl_compare {
    ($($t:ty, $o:ident, $slice:expr;)*) => {$(
        impl PartialEq<$t> for NonEmptyBytesMut {
            #[inline]
            fn eq(&self, $o: &$t) -> bool {
                self.raw() == $slice
            }
        }
        impl PartialEq<NonEmptyBytesMut> for $t {
            #[inline]
            fn eq(&self, other: &NonEmptyBytesMut) -> bool {
                let $o = self;
                $slice == other.raw()
            }
        }
        impl PartialOrd<$t> for NonEmptyBytesMut {
            #[inline]
            fn partial_cmp(&self, $o: &$t) -> Option<Ordering> {
                self.raw().partial_cmp($slice)
            }
        }
        impl PartialOrd<NonEmptyBytesMut> for $t {
            #[inline]
            fn partial_cmp(&self, other: &NonEmptyBytesMut) -> Option<Ordering> {
                let $o = self;
                $slice.partial_cmp(other.raw())
            }
        }
    )*};
}

impl_compare! {
    BytesMut, o, &o[..];
    Bytes, o, &o[..];
    [u8], o, o;
    &[u8], o, &o[..];
    str, o, o.as_bytes();
    &str, o, o.as_bytes();
    Vec<u8>, o, &o[..];
    String, o, o.as_bytes();
}
