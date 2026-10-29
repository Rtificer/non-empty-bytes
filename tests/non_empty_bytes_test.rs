//! Tests for `NonEmptyBytes`.
//!
//! These only exercise the wrapper's own behaviour (the non-empty invariant,
//! the `Option`/`Result` returns, the `NonZeroUsize` plumbing, trait impls).
//! The behaviour of `bytes::Bytes` itself is assumed to be correct.

use std::borrow::Borrow;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

use bytes::{Bytes, BytesMut};
use non_empty_bytes::{EmptyBytesError, NonEmptyBytes, NonEmptyBytesMut};

// ---------------------------------------------------------------- helpers

fn nz(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).expect("test helper requires a non-zero value")
}

fn neb(data: &[u8]) -> NonEmptyBytes {
    NonEmptyBytes::copy_from_slice(data).expect("test helper requires non-empty data")
}

/// An owner that records when it is dropped.
struct Owner {
    data: Vec<u8>,
    dropped: Arc<AtomicBool>,
}

impl AsRef<[u8]> for Owner {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.dropped.store(true, AtomicOrdering::SeqCst);
    }
}

// ------------------------------------------------------------ constructors

#[test]
fn new_rejects_empty_bytes() {
    assert!(NonEmptyBytes::new(Bytes::new()).is_none());
}

#[test]
fn new_rejects_empty_slice_of_non_empty_allocation() {
    let empty_view = Bytes::from_static(b"hello").slice(2..2);
    assert!(NonEmptyBytes::new(empty_view).is_none());
}

#[test]
fn new_accepts_non_empty_bytes() {
    let x = NonEmptyBytes::new(Bytes::from_static(b"abc")).unwrap();
    assert_eq!(&x[..], b"abc");
}

#[test]
fn new_accepts_single_byte() {
    let x = NonEmptyBytes::new(Bytes::from_static(b"z")).unwrap();
    assert_eq!(&x[..], b"z");
    assert_eq!(x.len(), nz(1));
}

#[test]
fn new_unchecked_wraps_non_empty_bytes() {
    // SAFETY: the buffer is non-empty.
    let x = unsafe { NonEmptyBytes::new_unchecked(Bytes::from_static(b"abc")) };
    assert_eq!(&x[..], b"abc");
}

#[cfg(debug_assertions)]
#[test]
#[should_panic]
fn new_unchecked_debug_asserts_on_empty_input() {
    // Deliberately violates the safety contract. The `debug_assert!` fires
    // before any invalid value can escape, so this is only meaningful (and
    // only compiled) when debug assertions are enabled.
    let _ = unsafe { NonEmptyBytes::new_unchecked(Bytes::new()) };
}

#[test]
fn from_static_rejects_empty() {
    assert!(NonEmptyBytes::from_static(b"").is_none());
}

#[test]
fn from_static_accepts_non_empty() {
    let x = NonEmptyBytes::from_static(b"hello").unwrap();
    assert_eq!(&x[..], b"hello");
}

#[test]
fn from_static_does_not_copy() {
    static DATA: &[u8] = b"hello";
    let x = NonEmptyBytes::from_static(DATA).unwrap();
    assert_eq!(x.as_ptr(), DATA.as_ptr());
}

#[test]
fn from_static_is_usable_in_const_context() {
    const GREETING: Option<NonEmptyBytes> = NonEmptyBytes::from_static(b"hi");
    const NOTHING: Option<NonEmptyBytes> = NonEmptyBytes::from_static(b"");
    assert_eq!(&GREETING.unwrap()[..], b"hi");
    assert!(NOTHING.is_none());
}

#[test]
fn copy_from_slice_rejects_empty() {
    assert!(NonEmptyBytes::copy_from_slice(&[]).is_none());
}

#[test]
fn copy_from_slice_copies_the_data() {
    let src = vec![1u8, 2, 3];
    let x = NonEmptyBytes::copy_from_slice(&src).unwrap();
    assert_eq!(&x[..], &src[..]);
    assert_ne!(
        x.as_ptr(),
        src.as_ptr(),
        "data must be copied, not borrowed"
    );
}

#[test]
fn from_owner_rejects_empty_owner() {
    assert!(NonEmptyBytes::from_owner(Vec::<u8>::new()).is_none());
}

#[test]
fn from_owner_drops_empty_owner() {
    let dropped = Arc::new(AtomicBool::new(false));
    let owner = Owner {
        data: Vec::new(),
        dropped: Arc::clone(&dropped),
    };
    assert!(NonEmptyBytes::from_owner(owner).is_none());
    assert!(
        dropped.load(AtomicOrdering::SeqCst),
        "rejected owner must not leak"
    );
}

#[test]
fn from_owner_exposes_owner_data() {
    let x = NonEmptyBytes::from_owner(vec![1u8, 2, 3]).unwrap();
    assert_eq!(&x[..], &[1u8, 2, 3]);
}

#[test]
fn from_owner_keeps_owner_alive_until_last_clone_is_dropped() {
    let dropped = Arc::new(AtomicBool::new(false));
    let owner = Owner {
        data: b"hello".to_vec(),
        dropped: Arc::clone(&dropped),
    };

    let x = NonEmptyBytes::from_owner(owner).unwrap();
    assert_eq!(&x[..], b"hello");

    let clone = x.clone();
    drop(x);
    assert!(!dropped.load(AtomicOrdering::SeqCst));

    drop(clone);
    assert!(dropped.load(AtomicOrdering::SeqCst));
}

// ------------------------------------------------------ inner / accessors

#[test]
fn as_inner_returns_the_wrapped_bytes() {
    let x = neb(b"hello  world");
    let inner: &Bytes = x.as_inner();
    assert_eq!(&inner[..], b"hello  world");
    assert_eq!(&inner.slice(..5)[..], b"hello");
}

#[test]
fn into_inner_returns_the_wrapped_bytes_and_allows_emptying() {
    let x = neb(b"hello");
    let mut inner: Bytes = x.into_inner();
    let head = inner.split_to(5);
    assert_eq!(&head[..], b"hello");
    assert!(inner.is_empty());
}

#[test]
fn len_is_non_zero_and_correct() {
    assert_eq!(neb(b"x").len(), nz(1));
    assert_eq!(neb(b"hello").len(), nz(5));
    assert_eq!(neb(b"hello").len().get(), 5);
}

#[test]
fn len_tracks_changes_in_length() {
    let mut x = neb(b"hello world");
    x.truncate(nz(5));
    assert_eq!(x.len(), nz(5));
}

// ------------------------------------------------------------- is_unique

#[test]
fn is_unique_is_true_for_sole_owner_of_vec_backed_buffer() {
    let x = NonEmptyBytes::try_from(vec![1u8, 2, 3]).unwrap();
    assert!(x.is_unique());
}

#[test]
fn is_unique_is_false_while_a_clone_exists() {
    let x = NonEmptyBytes::try_from(vec![1u8, 2, 3]).unwrap();
    let y = x.clone();
    assert!(!x.is_unique());
    assert!(!y.is_unique());
    drop(y);
    assert!(x.is_unique());
}

#[test]
fn is_unique_is_false_for_static_data() {
    assert!(!NonEmptyBytes::from_static(b"hello").unwrap().is_unique());
}

#[test]
fn is_unique_is_false_for_owner_backed_data() {
    assert!(
        !NonEmptyBytes::from_owner(vec![1u8, 2, 3])
            .unwrap()
            .is_unique()
    );
}

// ----------------------------------------------------------------- slice

#[test]
fn slice_returns_requested_subrange() {
    let x = neb(b"hello world");
    assert_eq!(&x.slice(2..5).unwrap()[..], b"llo");
    assert_eq!(&x.slice(..5).unwrap()[..], b"hello");
    assert_eq!(&x.slice(6..).unwrap()[..], b"world");
    assert_eq!(&x.slice(1..=2).unwrap()[..], b"el");
    assert_eq!(&x.slice(..).unwrap()[..], b"hello world");
}

#[test]
fn slice_returns_none_for_empty_ranges() {
    let x = neb(b"hello");
    assert!(x.slice(2..2).is_none());
    assert!(x.slice(0..0).is_none());
    assert!(x.slice(5..5).is_none());
    assert!(x.slice(5..).is_none());
    assert!(x.slice(..0).is_none());
}

#[test]
fn slice_can_yield_a_single_byte() {
    let x = neb(b"hello");
    assert_eq!(&x.slice(4..).unwrap()[..], b"o");
    assert_eq!(&x.slice(..1).unwrap()[..], b"h");
}

#[test]
fn slice_shares_memory_with_the_original() {
    let x = neb(b"hello world");
    let s = x.slice(2..5).unwrap();
    assert_eq!(s.as_ptr(), x.as_ptr().wrapping_add(2));
}

#[test]
fn slice_leaves_the_original_untouched() {
    let x = neb(b"hello");
    let _ = x.slice(1..3);
    assert_eq!(&x[..], b"hello");
}

#[test]
#[should_panic]
fn slice_panics_when_end_is_out_of_bounds() {
    let _ = neb(b"hello").slice(0..6);
}

#[test]
#[should_panic]
#[allow(clippy::reversed_empty_ranges)]
fn slice_panics_when_begin_is_after_end() {
    let _ = neb(b"hello").slice(4..2);
}

// ------------------------------------------------------------- slice_ref

#[test]
fn slice_ref_returns_the_matching_subset() {
    let x = neb(b"012345678");
    let subset = &x[2..6];
    let sub = x.slice_ref(subset).unwrap();
    assert_eq!(&sub[..], b"2345");
    assert_eq!(sub.as_ptr(), subset.as_ptr());
}

#[test]
fn slice_ref_of_whole_buffer_returns_whole_buffer() {
    let x = neb(b"012345678");
    let whole = x.slice_ref(&x[..]).unwrap();
    assert_eq!(whole, x);
}

#[test]
fn slice_ref_returns_none_for_empty_subset() {
    let x = neb(b"012345678");
    assert!(x.slice_ref(&x[2..2]).is_none());
    assert!(x.slice_ref(&[]).is_none());
}

#[test]
#[should_panic]
fn slice_ref_panics_for_a_slice_from_another_buffer() {
    let x = neb(b"hello");
    let foreign = [1u8, 2, 3];
    let _ = x.slice_ref(&foreign);
}

// ------------------------------------------------------------- split_off

#[test]
fn split_off_splits_into_two_non_empty_halves() {
    let mut x = neb(b"hello world");
    let tail = x.split_off(nz(5)).unwrap();
    assert_eq!(&x[..], b"hello");
    assert_eq!(&tail[..], b" world");
}

#[test]
fn split_off_at_one_leaves_a_single_byte_in_self() {
    let mut x = neb(b"abc");
    let tail = x.split_off(nz(1)).unwrap();
    assert_eq!(&x[..], b"a");
    assert_eq!(&tail[..], b"bc");
}

#[test]
fn split_off_at_len_minus_one_leaves_a_single_byte_in_tail() {
    let mut x = neb(b"abc");
    let tail = x.split_off(nz(2)).unwrap();
    assert_eq!(&x[..], b"ab");
    assert_eq!(&tail[..], b"c");
}

#[test]
fn split_off_at_len_returns_none_and_leaves_self_unchanged() {
    let mut x = neb(b"hello");
    assert!(x.split_off(nz(5)).is_none());
    assert_eq!(&x[..], b"hello");
}

#[test]
fn split_off_past_len_returns_none_and_leaves_self_unchanged() {
    let mut x = neb(b"hello");
    assert!(x.split_off(nz(6)).is_none());
    assert!(x.split_off(nz(usize::MAX)).is_none());
    assert_eq!(&x[..], b"hello");
}

#[test]
fn split_off_of_single_byte_always_returns_none() {
    let mut x = neb(b"a");
    assert!(x.split_off(nz(1)).is_none());
    assert!(x.split_off(nz(2)).is_none());
    assert_eq!(&x[..], b"a");
}

#[test]
fn split_off_does_not_move_memory() {
    let mut x = neb(b"hello world");
    let base = x.as_ptr();
    let tail = x.split_off(nz(5)).unwrap();
    assert_eq!(x.as_ptr(), base);
    assert_eq!(tail.as_ptr(), base.wrapping_add(5));
}

#[test]
fn split_off_exhaustive_small_cases() {
    let data: Vec<u8> = (0..8).collect();
    for len in 1..=data.len() {
        for at in 1..=data.len() + 2 {
            let mut x = neb(&data[..len]);
            let result = x.split_off(nz(at));
            if at < len {
                let tail = result.expect("at < len must succeed");
                assert_eq!(&x[..], &data[..at], "len={len} at={at}");
                assert_eq!(&tail[..], &data[at..len], "len={len} at={at}");
            } else {
                assert!(result.is_none(), "len={len} at={at}");
                assert_eq!(&x[..], &data[..len], "len={len} at={at}");
            }
        }
    }
}

// -------------------------------------------------------------- split_to

#[test]
fn split_to_splits_into_two_non_empty_halves() {
    let mut x = neb(b"hello world");
    let head = x.split_to(nz(5)).unwrap();
    assert_eq!(&x[..], b" world");
    assert_eq!(&head[..], b"hello");
}

#[test]
fn split_to_at_one_returns_a_single_byte_head() {
    let mut x = neb(b"abc");
    let head = x.split_to(nz(1)).unwrap();
    assert_eq!(&head[..], b"a");
    assert_eq!(&x[..], b"bc");
}

#[test]
fn split_to_at_len_minus_one_leaves_a_single_byte_in_self() {
    let mut x = neb(b"abc");
    let head = x.split_to(nz(2)).unwrap();
    assert_eq!(&head[..], b"ab");
    assert_eq!(&x[..], b"c");
}

#[test]
fn split_to_at_len_returns_none_and_leaves_self_unchanged() {
    let mut x = neb(b"hello");
    assert!(x.split_to(nz(5)).is_none());
    assert_eq!(&x[..], b"hello");
}

#[test]
fn split_to_past_len_returns_none_and_leaves_self_unchanged() {
    let mut x = neb(b"hello");
    assert!(x.split_to(nz(6)).is_none());
    assert!(x.split_to(nz(usize::MAX)).is_none());
    assert_eq!(&x[..], b"hello");
}

#[test]
fn split_to_of_single_byte_always_returns_none() {
    let mut x = neb(b"a");
    assert!(x.split_to(nz(1)).is_none());
    assert_eq!(&x[..], b"a");
}

#[test]
fn split_to_does_not_move_memory() {
    let mut x = neb(b"hello world");
    let base = x.as_ptr();
    let head = x.split_to(nz(5)).unwrap();
    assert_eq!(head.as_ptr(), base);
    assert_eq!(x.as_ptr(), base.wrapping_add(5));
}

#[test]
fn split_to_exhaustive_small_cases() {
    let data: Vec<u8> = (0..8).collect();
    for len in 1..=data.len() {
        for at in 1..=data.len() + 2 {
            let mut x = neb(&data[..len]);
            let result = x.split_to(nz(at));
            if at < len {
                let head = result.expect("at < len must succeed");
                assert_eq!(&head[..], &data[..at], "len={len} at={at}");
                assert_eq!(&x[..], &data[at..len], "len={len} at={at}");
            } else {
                assert!(result.is_none(), "len={len} at={at}");
                assert_eq!(&x[..], &data[..len], "len={len} at={at}");
            }
        }
    }
}

// -------------------------------------------------------------- truncate

#[test]
fn truncate_shortens_the_buffer() {
    let mut x = neb(b"hello world");
    x.truncate(nz(5));
    assert_eq!(&x[..], b"hello");
    assert_eq!(x, b"hello"[..]);
}

#[test]
fn truncate_to_one_keeps_the_first_byte() {
    let mut x = neb(b"hello");
    x.truncate(nz(1));
    assert_eq!(&x[..], b"h");
    assert_eq!(x.len(), nz(1));
}

#[test]
fn truncate_to_current_len_is_a_no_op() {
    let mut x = neb(b"hello");
    x.truncate(nz(5));
    assert_eq!(&x[..], b"hello");
}

#[test]
fn truncate_beyond_len_is_a_no_op() {
    let mut x = neb(b"hello");
    x.truncate(nz(100));
    assert_eq!(&x[..], b"hello");
    x.truncate(nz(usize::MAX));
    assert_eq!(&x[..], b"hello");
}

#[test]
fn truncate_never_leaves_the_buffer_empty() {
    let mut x = neb(b"hello");
    for len in (1..=5).rev() {
        x.truncate(nz(len));
        assert_eq!(x.len(), nz(len));
    }
    assert_eq!(&x[..], b"h");
}

// ----------------------------------------------------------- try_into_mut

#[test]
fn try_into_mut_succeeds_for_unique_buffer() {
    let x = NonEmptyBytes::try_from(b"hello".to_vec()).unwrap();
    let m = x.try_into_mut().expect("unique buffer should convert");
    assert_eq!(&m[..], b"hello");
}

#[test]
fn try_into_mut_result_is_mutable_and_non_empty() {
    let x = NonEmptyBytes::try_from(b"hello".to_vec()).unwrap();
    let mut m = x.try_into_mut().unwrap();
    m[0] = b'j';
    m.extend_from_slice(b"!");
    assert_eq!(&m[..], b"jello!");
}

#[test]
fn try_into_mut_fails_and_returns_self_when_shared() {
    let x = NonEmptyBytes::try_from(b"hello".to_vec()).unwrap();
    let other = x.clone();
    let returned = x
        .try_into_mut()
        .expect_err("shared buffer must not convert");
    assert_eq!(returned, other);
    assert_eq!(&returned[..], b"hello");
}

#[test]
fn try_into_mut_fails_for_static_data() {
    let x = NonEmptyBytes::from_static(b"hello").unwrap();
    let returned = x.try_into_mut().expect_err("static data must not convert");
    assert_eq!(&returned[..], b"hello");
}

#[test]
fn try_into_mut_fails_for_owner_backed_data() {
    let x = NonEmptyBytes::from_owner(b"hello".to_vec()).unwrap();
    let returned = x
        .try_into_mut()
        .expect_err("owner-backed data must not convert");
    assert_eq!(&returned[..], b"hello");
}

#[test]
fn try_into_mut_succeeds_again_once_clones_are_dropped() {
    let x = NonEmptyBytes::try_from(b"hello".to_vec()).unwrap();
    let other = x.clone();
    let x = x.try_into_mut().expect_err("still shared");
    drop(other);
    assert!(x.try_into_mut().is_ok());
}

// ------------------------------------------------------------ trait impls

#[test]
fn as_ref_and_deref_expose_the_bytes() {
    let x = neb(b"abc");
    assert_eq!(AsRef::<[u8]>::as_ref(&x), b"abc");
    let derefed: &[u8] = &x;
    assert_eq!(derefed, b"abc");
    assert_eq!(x.first(), Some(&b'a'));
    assert_eq!(x.last(), Some(&b'c'));
}

#[test]
fn borrow_exposes_the_bytes() {
    let x = neb(b"abc");
    let borrowed: &[u8] = <NonEmptyBytes as Borrow<[u8]>>::borrow(&x);
    assert_eq!(borrowed, b"abc");
}

#[test]
fn into_iterator_by_value_yields_every_byte_in_order() {
    let collected: Vec<u8> = neb(&[1u8, 2, 3]).into_iter().collect();
    assert_eq!(collected, vec![1u8, 2, 3]);
}

#[test]
fn into_iterator_by_reference_yields_every_byte_in_order() {
    let x = neb(&[1u8, 2, 3]);
    let mut seen: Vec<u8> = Vec::new();
    for byte in &x {
        seen.push(*byte);
    }
    assert_eq!(seen, vec![1u8, 2, 3]);
    // iterating by reference does not consume the value
    assert_eq!(&x[..], &[1u8, 2, 3]);
}

#[test]
fn clone_is_equal_and_shares_the_allocation() {
    let x = neb(b"hello");
    let y = x.clone();
    assert_eq!(x, y);
    assert_eq!(x.as_ptr(), y.as_ptr());
}

#[test]
fn debug_output_names_the_type() {
    let rendered = format!("{:?}", neb(b"a"));
    assert!(rendered.starts_with("NonEmptyBytes("), "got: {rendered}");
}

#[test]
fn is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<NonEmptyBytes>();
}

// ------------------------------------------------------------ conversions

#[test]
fn converts_into_bytes() {
    let converted = Bytes::from(neb(b"hello"));
    assert_eq!(&converted[..], b"hello");
}

#[test]
fn converts_into_bytes_mut() {
    let converted = BytesMut::from(neb(b"hello"));
    assert_eq!(&converted[..], b"hello");
}

#[test]
fn converts_into_vec() {
    let converted = Vec::<u8>::from(neb(b"hello"));
    assert_eq!(converted, b"hello".to_vec());
}

#[test]
fn converts_into_non_empty_bytes_mut() {
    let converted = NonEmptyBytesMut::from(neb(b"hello"));
    assert_eq!(&converted[..], b"hello");
}

// --------------------------------------------------------------- TryFrom

#[test]
fn try_from_accepts_non_empty_values_of_every_supported_type() {
    let from_bytes = NonEmptyBytes::try_from(Bytes::from_static(b"ab")).unwrap();
    assert_eq!(&from_bytes[..], b"ab");

    let from_slice = NonEmptyBytes::try_from(&b"ab"[..]).unwrap();
    assert_eq!(&from_slice[..], b"ab");

    let from_str = NonEmptyBytes::try_from("ab").unwrap();
    assert_eq!(&from_str[..], b"ab");

    let from_vec = NonEmptyBytes::try_from(vec![b'a', b'b']).unwrap();
    assert_eq!(&from_vec[..], b"ab");

    let from_box = NonEmptyBytes::try_from(vec![b'a', b'b'].into_boxed_slice()).unwrap();
    assert_eq!(&from_box[..], b"ab");

    let from_string = NonEmptyBytes::try_from(String::from("ab")).unwrap();
    assert_eq!(&from_string[..], b"ab");
}

#[test]
fn try_from_rejects_empty_values_of_every_supported_type() {
    let empty_slice: &'static [u8] = b"";
    let empty_str: &'static str = "";

    assert!(matches!(
        NonEmptyBytes::try_from(Bytes::new()),
        Err(EmptyBytesError)
    ));
    assert!(matches!(
        NonEmptyBytes::try_from(empty_slice),
        Err(EmptyBytesError)
    ));
    assert!(matches!(
        NonEmptyBytes::try_from(empty_str),
        Err(EmptyBytesError)
    ));
    assert!(matches!(
        NonEmptyBytes::try_from(Vec::<u8>::new()),
        Err(EmptyBytesError)
    ));
    assert!(matches!(
        NonEmptyBytes::try_from(Vec::<u8>::new().into_boxed_slice()),
        Err(EmptyBytesError)
    ));
    assert!(matches!(
        NonEmptyBytes::try_from(String::new()),
        Err(EmptyBytesError)
    ));
}

// ----------------------------------------------------------- comparisons

#[test]
fn eq_with_every_supported_type_in_both_directions() {
    let x = neb(b"abc");

    assert!(x == Bytes::from_static(b"abc"));
    assert!(Bytes::from_static(b"abc") == x);

    assert!(x == b"abc"[..]);
    assert!(b"abc"[..] == x);

    assert!(x == &b"abc"[..]);
    assert!(&b"abc"[..] == x);

    assert!(x == *"abc");
    assert!(*"abc" == x);

    assert!(x == "abc");
    assert!("abc" == x);

    assert!(x == b"abc".to_vec());
    assert!(b"abc".to_vec() == x);

    assert!(x == String::from("abc"));
    assert!(String::from("abc") == x);
}

#[test]
fn ne_with_every_supported_type_in_both_directions() {
    let x = neb(b"abc");

    assert!(x != Bytes::from_static(b"abd"));
    assert!(Bytes::from_static(b"abd") != x);

    assert!(x != b"abd"[..]);
    assert!(b"abd"[..] != x);

    assert!(x != &b"abd"[..]);
    assert!(&b"abd"[..] != x);

    assert!(x != *"abd");
    assert!(*"abd" != x);

    assert!(x != "abd");
    assert!("abd" != x);

    assert!(x != b"abd".to_vec());
    assert!(b"abd".to_vec() != x);

    assert!(x != String::from("abd"));
    assert!(String::from("abd") != x);
}

#[test]
fn eq_distinguishes_different_lengths() {
    let x = neb(b"abc");
    assert!(x != "ab");
    assert!(x != "abcd");
    assert!(x != "");
    assert!("" != x);
}

#[test]
fn partial_ord_with_every_supported_type_in_both_directions() {
    let x = neb(b"abc");

    assert!(x < Bytes::from_static(b"abd"));
    assert!(Bytes::from_static(b"abd") > x);

    assert!(x < b"abd"[..]);
    assert!(b"abd"[..] > x);

    assert!(x < &b"abd"[..]);
    assert!(&b"abd"[..] > x);

    assert!(x < *"abd");
    assert!(*"abd" > x);

    assert!(x < "abd");
    assert!("abd" > x);

    assert!(x < b"abd".to_vec());
    assert!(b"abd".to_vec() > x);

    assert!(x < String::from("abd"));
    assert!(String::from("abd") > x);
}

#[test]
fn partial_cmp_reports_equal_for_equal_contents() {
    use std::cmp::Ordering;
    let x = neb(b"abc");
    assert_eq!(
        x.partial_cmp(&Bytes::from_static(b"abc")),
        Some(Ordering::Equal)
    );
    assert_eq!(x.partial_cmp(&"abc"), Some(Ordering::Equal));
    assert_eq!(x.partial_cmp(&b"abd".to_vec()), Some(Ordering::Less));
    assert_eq!(x.partial_cmp(&String::from("abb")), Some(Ordering::Greater));
}

#[test]
fn ordering_is_lexicographic_and_prefix_is_smaller() {
    assert!(neb(b"ab") < neb(b"abc"));
    assert!(neb(b"abc") < neb(b"abd"));
    assert!(neb(b"b") > neb(b"abc"));
    assert_eq!(neb(b"abc").cmp(&neb(b"abc")), std::cmp::Ordering::Equal);
}

#[test]
fn sorting_uses_byte_order() {
    let mut values = vec![neb(b"b"), neb(b"abc"), neb(b"ab"), neb(b"a")];
    values.sort();
    let sorted: Vec<&[u8]> = values.iter().map(|v| &v[..]).collect();
    assert_eq!(sorted, vec![&b"a"[..], &b"ab"[..], &b"abc"[..], &b"b"[..]]);
}

#[test]
fn equality_ignores_how_the_value_was_built() {
    let from_static = NonEmptyBytes::from_static(b"abc").unwrap();
    let from_vec = NonEmptyBytes::try_from(b"abc".to_vec()).unwrap();
    let from_slice = neb(b"zabcz").slice(1..4).unwrap();
    assert_eq!(from_static, from_vec);
    assert_eq!(from_vec, from_slice);
}
