use crate::{EmptyBytesError, NonEmptyBytesMut};
use alloc::{boxed::Box, string::String, vec::Vec};
use bytes::{Bytes, BytesMut};
use core::borrow::Borrow;
use core::cmp::Ordering;
use core::num::NonZeroUsize;
use core::ops::Deref;
use core::ops::RangeBounds;

/// An immutable, cheaply cloneable byte buffer that always contains at least one byte.
///
/// `NonEmptyBytes` wraps a [`Bytes`] and adds exactly one guarantee: [`len`](Self::len) is
/// never zero. Everything else is inherited from [`Bytes`]. The storage is reference counted, so
/// cloning, [`slice`](Self::slice), [`split_off`](Self::split_off) and
/// [`split_to`](Self::split_to) are `O(1)` and never copy the data.
///
/// Use [`NonEmptyBytesMut`] to build or modify contents;
/// [`freeze`](NonEmptyBytesMut::freeze) turns one into a [`NonEmptyBytes`] at no cost.
///
/// # How the guarantee is kept
///
/// Operations that could produce an empty buffer either return an [`Option`]
/// ([`slice`](Self::slice), [`slice_ref`](Self::slice_ref), [`split_off`](Self::split_off),
/// [`split_to`](Self::split_to) and the constructors) or require a [`NonZeroUsize`]
/// ([`truncate`](Self::truncate) and the split points). There is no [`clear`](Bytes::clear), and the type does
/// not implement [`bytes::Buf`], since [`bytes::Buf::advance`] would break the guarantee. The wrapped
/// [`Bytes`] is only reachable by reference ([`as_inner`](Self::as_inner)) or by giving up the
/// wrapper ([`into_inner`](Self::into_inner)).
///
/// # Creating a value
///
/// * [`from_static`](Self::from_static): from a [`&'static [u8]`], with no allocation or copy.
///   This is a `const fn`.
/// * [`copy_from_slice`](Self::copy_from_slice): from any borrowed slice, by copying it.
/// * [`from_owner`](Self::from_owner): zero-copy over memory kept alive by an owner, such as a
///   memory map.
/// * [`new`](Self::new): from an existing [`Bytes`].
/// * [`TryFrom`] from [`Bytes`], [`&'static [u8]`], [`&'static str`], [`Vec<u8>`], [`Box<[u8]>`] and
///   [`String`], failing with [`EmptyBytesError`] if the input is empty. Owned inputs reuse their
///   allocation.
/// * [`new_unchecked`](Self::new_unchecked): `unsafe`, skips the check.
///
/// The constructors that can fail return [`None`] when the input is empty.
///
/// # Reading the data
///
/// `NonEmptyBytes` implements [`Deref`] to [`[u8]`], along with [`AsRef<[u8]>`](AsRef),
/// [`Borrow<[u8]>`](Borrow) and [`IntoIterator`] (yielding `u8` by value and `&u8` by
/// reference), so the whole slice API is available. The inherent [`len`](Self::len) takes
/// precedence over the slice method and returns a [`NonZeroUsize`].
///
/// # Converting
///
/// * To a mutable buffer: [`try_into_mut`](Self::try_into_mut) never copies but fails if the
///   buffer is shared, static or owner-backed. `NonEmptyBytesMut::from` always succeeds, but
///   copies in those cases.
/// * To a plain type: [`into_inner`](Self::into_inner), or the `From` impls for [`Bytes`],
///   [`BytesMut`] and [`Vec<u8>`]. The guarantee no longer applies to the result, and the
///   [`BytesMut`] and [`Vec<u8>`] conversions copy unless the buffer is uniquely owned.
///
/// # Comparisons
///
/// Equality and ordering are bytewise. A `NonEmptyBytes` can be compared directly, in either
/// order, with [`Bytes`], [`[u8]`], [`&[u8]`], [`str`], [`&str`], [`Vec<u8>`] and [`String`].
///
/// There is deliberately no [`Default`] implementation, since the default value would be empty.
///
/// # Examples
///
/// ```
/// use core::num::NonZeroUsize;
/// use non_empty_bytes::NonEmptyBytes;
///
/// let mut msg = NonEmptyBytes::try_from(&b"hello world"[..]).unwrap();
///
/// // Reading works like any byte slice, through [`Deref<Target = [u8]>`].
/// assert_eq!(msg[0], b'h');
/// assert!(msg.starts_with(b"hello"));
///
/// // The length can never be zero, so it is a [`NonZeroUsize`].
/// assert_eq!(msg.len().get(), 11);
///
/// // Splitting returns an `Option`, because one half could be empty.
/// let head = msg.split_to(NonZeroUsize::new(6).unwrap()).unwrap();
/// assert_eq!(&head[..], b"hello ");
/// assert_eq!(&msg[..], b"world");
///
/// // Empty input is rejected.
/// assert!(NonEmptyBytes::copy_from_slice(b"").is_none());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NonEmptyBytes(Bytes);

impl NonEmptyBytes {
    #[inline]
    #[must_use]
    pub fn new(bytes: Bytes) -> Option<Self> {
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
    pub const unsafe fn new_unchecked(bytes: Bytes) -> Self {
        debug_assert!(!bytes.is_empty());
        Self(bytes)
    }

    /// Creates a new [`NonEmptyBytes`] from a static slice, or returns [`None`] if the slice is empty
    ///
    /// The returned [`NonEmptyBytes`] will point directly to the static slice. There is
    /// no allocating or copying.
    ///
    /// # Examples
    ///
    /// ```
    /// use non_empty_bytes::NonEmptyBytes;
    ///
    /// let b = NonEmptyBytes::from_static(b"hello").unwrap();
    /// assert_eq!(&b[..], b"hello");
    ///
    /// assert!(NonEmptyBytes::from_static(b"").is_none());
    ///
    /// const GREETING: Option<NonEmptyBytes> = NonEmptyBytes::from_static(b"hi");
    /// ```
    #[inline]
    #[must_use]
    pub const fn from_static(bytes: &'static [u8]) -> Option<Self> {
        if bytes.is_empty() {
            None
        } else {
            Some(Self(Bytes::from_static(bytes)))
        }
    }

    /// Creates [`NonEmptyBytes`] instance from slice, by copying it, or returns [`None`] if the
    /// slice is empty
    #[inline]
    #[must_use]
    pub fn copy_from_slice(data: &[u8]) -> Option<Self> {
        if data.is_empty() {
            None
        } else {
            Some(Self(Bytes::copy_from_slice(data)))
        }
    }

    /// Creates a [`NonEmptyBytes`] over a buffer whose lifetime is controlled
    /// via an explicit owner, or returns [`None`] if the owner's buffer is empty.
    ///
    /// A common use case is to zero-copy construct from mapped memory.
    ///
    /// ```
    /// # struct File;
    /// #
    /// # impl File {
    /// #     pub fn open(_: &str) -> Result<Self, ()> {
    /// #         Ok(Self)
    /// #     }
    /// # }
    /// #
    /// # mod memmap2 {
    /// #     pub struct Mmap;
    /// #
    /// #     impl Mmap {
    /// #         pub unsafe fn map(_file: &super::File) -> Result<Self, ()> {
    /// #             Ok(Self)
    /// #         }
    /// #     }
    /// #
    /// #     impl AsRef<[u8]> for Mmap {
    /// #         fn as_ref(&self) -> &[u8] {
    /// #             b"buf"
    /// #         }
    /// #     }
    /// # }
    /// use non_empty_bytes::NonEmptyBytes;
    /// use memmap2::Mmap;
    ///
    /// # fn main() -> Result<(), ()> {
    /// let file = File::open("upload_bundle.tar.gz")?;
    /// let mmap = unsafe { Mmap::map(&file) }?;
    /// let b = NonEmptyBytes::from_owner(mmap).unwrap();
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// The `owner` will be transferred to the constructed [`NonEmptyBytes`] object, which
    /// will ensure it is dropped once all remaining clones of the constructed
    /// object are dropped. The owner will then be responsible for dropping the
    /// specified region of memory as part of its [`Drop`] implementation.
    ///
    /// Note that converting [`NonEmptyBytes`] constructed from an owner into a [`NonEmptyBytesMut`]
    /// will always create a deep copy of the buffer into newly allocated memory.
    #[inline]
    #[must_use]
    pub fn from_owner<T>(owner: T) -> Option<Self>
    where
        T: AsRef<[u8]> + Send + 'static,
    {
        Self::new(Bytes::from_owner(owner))
    }

    /// Get a reference to the inner [`Bytes`] object.
    ///
    /// Commonly used to call read-only [`Bytes`] methods not exposed on the wrapper,
    /// to clone the inner value while keeping the wrapper, or to pass to apis.
    ///
    /// # Examples
    /// ```
    /// use bytes::Bytes;
    /// use non_empty_bytes::NonEmptyBytes;
    ///
    /// let b = NonEmptyBytes::from_static(b"hello  world").unwrap();
    /// let inner: &Bytes = b.as_inner();
    ///
    /// // use methods not exposed on `NonEmptyBytes`
    /// let hello = inner.slice(..5);
    /// assert_eq!(&hello[..], b"hello");
    /// ```
    #[inline]
    #[must_use]
    pub const fn as_inner(&self) -> &Bytes {
        &self.0
    }
    /// Consumes the wrapper, returning the inner [`Bytes`].
    ///
    /// The non-empty guarantee no longer applies to the returned value, so it
    /// can be freely mutated, e.g. with [`Bytes::split_to`] or [`bytes::Buf::advance`].
    ///
    /// # Examples
    ///
    /// ```
    /// use bytes::Bytes;
    /// use non_empty_bytes::NonEmptyBytes;
    ///
    /// let b = NonEmptyBytes::from_static(b"hello").unwrap();
    /// let mut inner: Bytes = b.into_inner();
    ///
    /// let head = inner.split_to(5);
    /// assert_eq!(&head[..], b"hello");
    /// assert!(inner.is_empty()); // the invariant no longer holds
    /// ```
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> Bytes {
        self.0
    }

    /// Returns the number of bytes contained in this [`NonEmptyBytes`].
    ///
    /// The length is returned as a [`NonZeroUsize`], since the buffer is never empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use non_empty_bytes::NonEmptyBytes;
    ///
    /// let b = NonEmptyBytes::try_from(&b"hello"[..]).unwrap();
    /// assert_eq!(b.len().get(), 5);
    /// ```
    #[inline]
    #[must_use]
    #[allow(clippy::len_without_is_empty)] // never empty
    pub const fn len(&self) -> NonZeroUsize {
        // SAFTEY: length of self is > 0. This is the entire invariant the class seeks to enforce.
        unsafe { NonZeroUsize::new_unchecked(self.0.len()) }
    }

    /// Returns true if this is the only reference to the data and
    /// [`Into<NonEmptyBytesMut>`] would avoid cloning the underlying buffer.
    ///
    /// Always returns false if the data is backed by a [static slice](NonEmptyBytes::from_static),
    /// or an [owner](NonEmptyBytes::from_owner).
    ///
    /// The result of this method may be invalidated immediately if another
    /// thread clones this value while this is being called. Ensure you have
    /// unique access to this value (`&mut NonEmptyBytes`) first if you need to be
    /// certain the result is valid (i.e. for saftey reasons).
    ///
    /// # Examples
    ///
    /// ```
    /// use non_empty_bytes::NonEmptyBytes;
    ///
    /// let a = NonEmptyBytes::try_from(vec![1, 2, 3]).unwrap();
    /// assert!(a.is_unique());
    /// let b = a.clone();
    /// assert!(!a.is_unique());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_unique(&self) -> bool {
        self.0.is_unique()
    }

    /// Returns a slice of self for the provided range, or [`None`] if the range
    /// is empty.
    ///
    /// This will increment the reference count for the underlying memory and
    /// return a new [`NonEmptyBytes`] handle set to the slice.
    ///
    /// This operation is `O(1)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use non_empty_bytes::NonEmptyBytes;
    ///
    /// let a = NonEmptyBytes::try_from(&b"hello world"[..]).unwrap();
    /// let b = a.slice(2..5).unwrap();
    ///
    /// assert_eq!(&b[..], b"llo");
    ///
    /// // an empty range yields `None`
    /// assert!(a.slice(2..2).is_none());
    /// ```
    ///
    /// # Panics
    ///
    /// Requires that `begin <= end` and `end <= self.len()`, otherwise slicing
    /// will panic.
    #[inline]
    #[must_use]
    pub fn slice(&self, range: impl RangeBounds<usize>) -> Option<Self> {
        Self::new(self.0.slice(range))
    }

    /// Returns a slice of self that is equivalent to the given `subset`, or
    /// [`None`] if `subset` is empty.
    ///
    /// When processing a [`NonEmptyBytes`] buffer with other tools, one often gets a
    /// `&[u8]` which is in fact a slice of the [`NonEmptyBytes`], i.e. a subset of it.
    /// This function turns that `&[u8]` into another [`NonEmptyBytes`], as if one had
    /// called `self.slice()` with the offsets that correspond to `subset`.
    ///
    /// This operation is `O(1)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use non_empty_bytes::NonEmptyBytes;
    ///
    /// let bytes = NonEmptyBytes::try_from(&b"012345678"[..]).unwrap();
    /// let as_slice: &[u8] = bytes.as_ref();
    /// let subset = &as_slice[2..6];
    /// let subslice = bytes.slice_ref(subset).unwrap();
    /// assert_eq!(&subslice[..], b"2345");
    ///
    /// // an empty subset yields `None`
    /// assert!(bytes.slice_ref(&as_slice[2..2]).is_none());
    /// ```
    ///
    /// # Panics
    ///
    /// Requires that the given `sub` slice is in fact contained within the
    /// [`NonEmptyBytes`] buffer; otherwise this function will panic.
    #[inline]
    #[must_use]
    pub fn slice_ref(&self, subset: &[u8]) -> Option<Self> {
        Self::new(self.0.slice_ref(subset))
    }

    /// Splits the bytes into two at the given index, or returns [`None`] if
    /// `at >= self.len()`.
    ///
    /// Afterwards `self` contains elements `[0, at)`, and the returned [`NonEmptyBytes`]
    /// contains elements `[at, len)`. Since `at` is non-zero and less than `len`, both
    /// halves are non-empty. If [`None`] is returned, `self` is left unchanged.
    /// It's guaranteed that the memory does not move, that is, the address of `self`
    /// does not change, and the address of the returned slice is `at` bytes after that.
    ///
    /// This is an `O(1)` operation that just increases the reference count and
    /// sets a few indices.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::num::NonZeroUsize;
    /// use non_empty_bytes::NonEmptyBytes;
    ///
    /// let mut a = NonEmptyBytes::try_from(&b"hello world"[..]).unwrap();
    /// let b = a.split_off(NonZeroUsize::new(5).unwrap()).unwrap();
    ///
    /// assert_eq!(&a[..], b"hello");
    /// assert_eq!(&b[..], b" world");
    ///
    /// // splitting at or past the end would leave the second half empty
    /// assert!(a.split_off(NonZeroUsize::new(5).unwrap()).is_none());
    /// ```
    #[must_use = "consider truncate if you don't need the other half"]
    pub fn split_off(&mut self, at: NonZeroUsize) -> Option<Self> {
        if at >= self.len() {
            return None;
        }
        Some(Self(self.0.split_off(at.get())))
    }

    /// Splits the bytes into two at the given index, or returns [`None`] if
    /// `at >= self.len()`.
    ///
    /// Afterwards `self` contains elements `[at, len)`, and the returned
    /// [`NonEmptyBytes`] contains elements `[0, at)`. Since `at` is non-zero and less
    /// than `len`, both halves are non-empty. If [`None`] is returned, `self` is
    /// left unchanged.
    ///
    /// This is an `O(1)` operation that just increases the reference count and
    /// sets a few indices.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::num::NonZeroUsize;
    /// use non_empty_bytes::NonEmptyBytes;
    ///
    /// let mut a = NonEmptyBytes::try_from(&b"hello world"[..]).unwrap();
    /// let b = a.split_to(NonZeroUsize::new(5).unwrap()).unwrap();
    ///
    /// assert_eq!(&a[..], b" world");
    /// assert_eq!(&b[..], b"hello");
    ///
    /// // splitting at or past the end would leave `self` empty
    /// assert!(a.split_to(NonZeroUsize::new(6).unwrap()).is_none());
    /// ```
    #[must_use = "consider discarding the prefix another way if you don't need it"]
    pub fn split_to(&mut self, at: NonZeroUsize) -> Option<Self> {
        if at >= self.len() {
            return None;
        }
        Some(unsafe { Self::new_unchecked(self.0.split_to(at.get())) })
    }

    /// Shortens the buffer, keeping the first `len` bytes and dropping the
    /// rest.
    ///
    /// `len` is a [`NonZeroUsize`], so the buffer always keeps at least one byte.
    /// If `len` is greater than the buffer's current length, this has no
    /// effect.
    ///
    /// The [split_off](`Self::split_off()`) method can emulate `truncate`, but this causes the
    /// excess bytes to be returned instead of dropped.
    ///
    /// # Examples
    ///
    /// ```
    /// use core::num::NonZeroUsize;
    /// use non_empty_bytes::NonEmptyBytes;
    ///
    /// let mut buf = NonEmptyBytes::try_from(&b"hello world"[..]).unwrap();
    /// buf.truncate(NonZeroUsize::new(5).unwrap());
    /// assert_eq!(buf, b"hello"[..]);
    /// ```
    #[inline]
    pub fn truncate(&mut self, len: NonZeroUsize) {
        self.0.truncate(len.get());
    }

    /// Try to convert self into `NonEmptyBytesMut`.
    ///
    /// If `self` is unique for the entire original buffer, this will succeed
    /// and return a `NonEmptyBytesMut` with the contents of `self` without copying.
    /// If `self` is not unique for the entire original buffer, this will fail
    /// and return self.
    ///
    /// This will also always fail if the buffer was constructed via either
    /// [from_owner](NonEmptyBytes::from_owner) or [from_static](NonEmptyBytes::from_static).
    ///
    /// # Examples
    ///
    /// ```
    /// use non_empty_bytes::{NonEmptyBytes, NonEmptyBytesMut};
    ///
    /// let bytes = NonEmptyBytes::try_from(b"hello".to_vec()).unwrap();
    /// assert_eq!(
    ///     bytes.try_into_mut(),
    ///     Ok(NonEmptyBytesMut::try_from(&b"hello"[..]).unwrap())
    /// );
    ///
    /// // a shared buffer cannot be converted without copying, so `self` is returned
    /// let bytes = NonEmptyBytes::try_from(b"hello".to_vec()).unwrap();
    /// let _other = bytes.clone();
    /// assert_eq!(bytes.clone().try_into_mut(), Err(bytes));
    /// ```
    pub fn try_into_mut(self) -> Result<NonEmptyBytesMut, NonEmptyBytes> {
        match self.0.try_into_mut() {
            // SAFTEY: same bytes as `self`, which is already non-empty
            Ok(m) => Ok(unsafe { NonEmptyBytesMut::new_unchecked(m) }),
            Err(b) => Err(NonEmptyBytes(b)),
        }
    }
}

impl AsRef<[u8]> for NonEmptyBytes {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Deref for NonEmptyBytes {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Borrow<[u8]> for NonEmptyBytes {
    #[inline]
    fn borrow(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl IntoIterator for NonEmptyBytes {
    type Item = u8;
    type IntoIter = bytes::buf::IntoIter<Bytes>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a NonEmptyBytes {
    type Item = &'a u8;
    type IntoIter = core::slice::Iter<'a, u8>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl From<NonEmptyBytes> for Bytes {
    fn from(b: NonEmptyBytes) -> Bytes {
        b.0
    }
}

impl From<NonEmptyBytes> for BytesMut {
    fn from(b: NonEmptyBytes) -> BytesMut {
        b.0.into()
    }
}

impl From<NonEmptyBytes> for Vec<u8> {
    fn from(b: NonEmptyBytes) -> Vec<u8> {
        b.0.into()
    }
}

macro_rules! impl_try_from {
    ($($t:ty),* $(,)?) => {$(
        impl TryFrom<$t> for NonEmptyBytes {
            type Error = EmptyBytesError;

            #[inline]
            fn try_from(value: $t) -> Result<Self, Self::Error> {
                Self::new(Bytes::from(value)).ok_or(EmptyBytesError)
            }

        }
    )*};
}

impl_try_from!(
    Bytes,
    &'static [u8],
    &'static str,
    Vec<u8>,
    Box<[u8]>,
    String
);

macro_rules! impl_compare {
    ($($t:ty, $o:ident, $slice:expr;)*) => {$(
        impl PartialEq<$t> for NonEmptyBytes {
            #[inline]
            fn eq(&self, $o: &$t) -> bool {
                self.as_ref() == $slice
            }
        }
        impl PartialEq<NonEmptyBytes> for $t {
            #[inline]
            fn eq(&self, other: &NonEmptyBytes) -> bool {
                let $o = self;
                $slice == other.as_ref()
            }
        }
        impl PartialOrd<$t> for NonEmptyBytes {
            #[inline]
            fn partial_cmp(&self, $o: &$t) -> Option<Ordering> {
                self.as_ref().partial_cmp($slice)
            }
        }
        impl PartialOrd<NonEmptyBytes> for $t {
            #[inline]
            fn partial_cmp(&self, other: &NonEmptyBytes) -> Option<Ordering> {
                let $o = self;
                $slice.partial_cmp(other.as_ref())
            }
        }
    )*};
}

impl_compare! {
    Bytes, o, &o[..];
    [u8], o, o;
    &[u8], o, &o[..];
    str, o, o.as_bytes();
    &str, o, o.as_bytes();
    Vec<u8>, o, &o[..];
    String, o, o.as_bytes();
}
