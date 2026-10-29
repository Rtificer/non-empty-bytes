//! Tests for `NonEmptyBytesMut`.
//!
//! These only exercise the wrapper's own behaviour (the non-empty invariant,
//! the `Option`/`Result` returns, the `NonZeroUsize` plumbing, trait impls).
//! The behaviour of `bytes::BytesMut` itself is assumed to be correct.

use std::borrow::{Borrow, BorrowMut};
use std::collections::HashSet;
use std::fmt::Write as _;
use std::num::NonZeroUsize;

use bytes::{Bytes, BytesMut};
use non_empty_bytes::{EmptyBytesError, NonEmptyBytes, NonEmptyBytesMut};

// ---------------------------------------------------------------- helpers

fn nz(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).expect("test helper requires a non-zero value")
}

fn nebm(data: &[u8]) -> NonEmptyBytesMut {
    NonEmptyBytesMut::copy_from_slice(data).expect("test helper requires non-empty data")
}

// ------------------------------------------------------------ constructors

#[test]
fn new_rejects_empty() {
    assert!(NonEmptyBytesMut::new(BytesMut::new()).is_none());
}

#[test]
fn new_rejects_empty_buffer_with_capacity() {
    assert!(NonEmptyBytesMut::new(BytesMut::with_capacity(64)).is_none());
}

#[test]
fn new_accepts_non_empty() {
    let m = NonEmptyBytesMut::new(BytesMut::from(&b"abc"[..])).unwrap();
    assert_eq!(&m[..], b"abc");
}

#[test]
fn new_accepts_single_byte() {
    let m = NonEmptyBytesMut::new(BytesMut::from(&b"z"[..])).unwrap();
    assert_eq!(m.len(), nz(1));
}

#[test]
fn new_unchecked_wraps_non_empty() {
    // SAFETY: the buffer is non-empty.
    let m = unsafe { NonEmptyBytesMut::new_unchecked(BytesMut::from(&b"abc"[..])) };
    assert_eq!(&m[..], b"abc");
}

#[cfg(debug_assertions)]
#[test]
#[should_panic]
fn new_unchecked_debug_asserts_on_empty_input() {
    // Deliberately violates the safety contract. The `debug_assert!` fires
    // before any invalid value can escape, so this is only meaningful (and
    // only compiled) when debug assertions are enabled.
    let _ = unsafe { NonEmptyBytesMut::new_unchecked(BytesMut::new()) };
}

#[test]
fn copy_from_slice_rejects_empty() {
    assert!(NonEmptyBytesMut::copy_from_slice(&[]).is_none());
}

#[test]
fn copy_from_slice_copies_the_data() {
    let src = vec![1u8, 2, 3];
    let mut m = NonEmptyBytesMut::copy_from_slice(&src).unwrap();
    assert_eq!(&m[..], &src[..]);
    assert_ne!(
        m.as_ptr(),
        src.as_ptr(),
        "data must be copied, not borrowed"
    );

    m[0] = 99;
    assert_eq!(
        src,
        vec![1u8, 2, 3],
        "mutating the copy must not touch the source"
    );
}

#[test]
fn zeroed_creates_the_requested_number_of_zero_bytes() {
    let z = NonEmptyBytesMut::zeroed(nz(42));
    assert_eq!(z.len(), nz(42));
    assert!(z.capacity().get() >= 42);
    assert!(z.iter().all(|&byte| byte == 0));
}

#[test]
fn zeroed_of_one_is_a_single_zero_byte() {
    let z = NonEmptyBytesMut::zeroed(nz(1));
    assert_eq!(&z[..], &[0u8]);
}

// ------------------------------------------------------------- accessors

#[test]
fn as_inner_returns_the_wrapped_buffer() {
    let m = nebm(b"hello");
    let inner: &BytesMut = m.as_inner();
    assert_eq!(&inner[..], b"hello");
    assert_eq!(inner.len(), 5);
}

#[test]
fn into_inner_returns_the_wrapped_buffer_and_allows_emptying() {
    let m = nebm(b"hello");
    let mut inner: BytesMut = m.into_inner();
    let head = inner.split_to(5);
    assert_eq!(&head[..], b"hello");
    assert!(inner.is_empty());
}

#[test]
fn len_is_non_zero_and_correct() {
    assert_eq!(nebm(b"x").len(), nz(1));
    assert_eq!(nebm(b"hello").len(), nz(5));
    assert_eq!(nebm(b"hello").len().get(), 5);
}

#[test]
fn capacity_is_at_least_len_and_grows_with_reserve() {
    let mut m = nebm(b"hello");
    assert!(m.capacity().get() >= 5);
    m.reserve(64);
    assert!(m.capacity().get() >= 69);
}

#[test]
fn first_and_last_byte_of_a_single_byte_buffer_are_the_same() {
    let m = nebm(b"x");
    assert_eq!(m.first_byte(), b'x');
    assert_eq!(m.last_byte(), b'x');
}

#[test]
fn first_and_last_byte_of_a_longer_buffer() {
    let m = nebm(b"abc");
    assert_eq!(m.first_byte(), b'a');
    assert_eq!(m.last_byte(), b'c');
}

#[test]
fn first_and_last_byte_reflect_mutation() {
    let mut m = nebm(b"abc");
    m[0] = b'x';
    m[2] = b'z';
    assert_eq!(m.first_byte(), b'x');
    assert_eq!(m.last_byte(), b'z');

    m.extend_from_slice(b"!");
    assert_eq!(m.last_byte(), b'!');
}

// ------------------------------------------- spare_capacity_mut / set_len

#[test]
fn spare_capacity_mut_covers_the_unused_capacity() {
    let mut m = nebm(&[9u8]);
    m.reserve(10);
    let expected = m.capacity().get() - m.len().get();
    let spare = m.spare_capacity_mut();
    assert_eq!(spare.len(), expected);
    assert!(spare.len() >= 10);
}

#[test]
fn spare_capacity_can_be_filled_and_committed_with_set_len() {
    let mut m = nebm(&[9u8]);
    m.reserve(10);

    let spare = m.spare_capacity_mut();
    spare[0].write(0);
    spare[1].write(1);
    spare[2].write(2);

    unsafe { m.set_len(nz(4)) };
    assert_eq!(&m[..], &[9u8, 0, 1, 2]);
}

#[test]
fn set_len_can_shrink_and_regrow_over_initialised_bytes() {
    let mut m = nebm(b"hello world");

    unsafe { m.set_len(nz(5)) };
    assert_eq!(&m[..], b"hello");
    assert_eq!(m.len(), nz(5));

    unsafe { m.set_len(nz(11)) };
    assert_eq!(&m[..], b"hello world");
    assert_eq!(m.len(), nz(11));
}

#[test]
fn set_len_to_one_keeps_the_first_byte() {
    let mut m = nebm(b"hello");
    unsafe { m.set_len(nz(1)) };
    assert_eq!(&m[..], b"h");
}

// ----------------------------------------------------------------- freeze

#[test]
fn freeze_preserves_the_contents() {
    let frozen: NonEmptyBytes = nebm(b"hello").freeze();
    assert_eq!(&frozen[..], b"hello");
    assert_eq!(frozen.len(), nz(5));
}

#[test]
fn freeze_reflects_earlier_mutations() {
    let mut m = nebm(b"hello");
    m.extend_from_slice(b" world");
    m[0] = b'j';
    let frozen = m.freeze();
    assert_eq!(&frozen[..], b"jello world");
}

#[test]
fn frozen_value_can_be_shared_across_threads() {
    let mut m = nebm(b"hello");
    m.extend_from_slice(b" world");
    let first = m.freeze();
    let second = first.clone();

    let handle = std::thread::spawn(move || {
        assert_eq!(&first[..], b"hello world");
    });

    assert_eq!(&second[..], b"hello world");
    handle.join().unwrap();
}

// --------------------------------------------------------------- truncate

#[test]
fn truncate_shortens_the_buffer() {
    let mut m = nebm(b"hello world");
    m.truncate(nz(5));
    assert_eq!(&m[..], b"hello");
    assert_eq!(m, b"hello"[..]);
}

#[test]
fn truncate_preserves_capacity() {
    let mut m = nebm(b"hello world");
    let capacity = m.capacity();
    m.truncate(nz(5));
    assert_eq!(m.capacity(), capacity);
}

#[test]
fn truncate_to_one_keeps_the_first_byte() {
    let mut m = nebm(b"hello");
    m.truncate(nz(1));
    assert_eq!(&m[..], b"h");
    assert_eq!(m.len(), nz(1));
}

#[test]
fn truncate_to_current_len_is_a_no_op() {
    let mut m = nebm(b"hello");
    m.truncate(nz(5));
    assert_eq!(&m[..], b"hello");
}

#[test]
fn truncate_beyond_len_is_a_no_op() {
    let mut m = nebm(b"hello");
    m.truncate(nz(100));
    assert_eq!(&m[..], b"hello");
    m.truncate(nz(usize::MAX));
    assert_eq!(&m[..], b"hello");
}

#[test]
fn truncate_never_leaves_the_buffer_empty() {
    let mut m = nebm(b"hello");
    for len in (1..=5).rev() {
        m.truncate(nz(len));
        assert_eq!(m.len(), nz(len));
    }
    assert_eq!(&m[..], b"h");
}

// ----------------------------------------------------------------- resize

#[test]
fn resize_grows_with_the_fill_value() {
    let mut m = nebm(&[0x1u8]);
    m.resize(nz(3), 0x1);
    assert_eq!(&m[..], &[0x1u8, 0x1, 0x1]);
}

#[test]
fn resize_shrinks_by_truncating() {
    let mut m = nebm(&[0x1u8, 0x1, 0x1]);
    m.resize(nz(2), 0x2);
    assert_eq!(&m[..], &[0x1u8, 0x1]);
}

#[test]
fn resize_after_shrink_uses_the_new_fill_value() {
    let mut m = nebm(&[0x1u8]);
    m.resize(nz(3), 0x1);
    m.resize(nz(2), 0x2);
    m.resize(nz(4), 0x3);
    assert_eq!(&m[..], &[0x1u8, 0x1, 0x3, 0x3]);
}

#[test]
fn resize_to_same_len_is_a_no_op() {
    let mut m = nebm(b"abc");
    m.resize(nz(3), b'x');
    assert_eq!(&m[..], b"abc");
}

#[test]
fn resize_down_to_one_keeps_the_buffer_non_empty() {
    let mut m = nebm(b"abc");
    m.resize(nz(1), b'x');
    assert_eq!(&m[..], b"a");
    assert_eq!(m.len(), nz(1));
}

// ---------------------------------------------------------------- split_to

#[test]
fn split_to_splits_into_two_non_empty_halves() {
    let mut m = nebm(b"hello world");
    let head = m.split_to(nz(5)).unwrap();
    assert_eq!(&head[..], b"hello");
    assert_eq!(&m[..], b" world");
}

#[test]
fn split_to_halves_are_independently_mutable() {
    let mut m = nebm(b"hello world");
    let mut head = m.split_to(nz(5)).unwrap();

    m[0] = b'!';
    head[0] = b'j';

    assert_eq!(&m[..], b"!world");
    assert_eq!(&head[..], b"jello");
}

#[test]
fn split_to_at_one_returns_a_single_byte_head() {
    let mut m = nebm(b"abc");
    let head = m.split_to(nz(1)).unwrap();
    assert_eq!(&head[..], b"a");
    assert_eq!(&m[..], b"bc");
}

#[test]
fn split_to_at_len_minus_one_leaves_a_single_byte_in_self() {
    let mut m = nebm(b"abc");
    let head = m.split_to(nz(2)).unwrap();
    assert_eq!(&head[..], b"ab");
    assert_eq!(&m[..], b"c");
}

#[test]
fn split_to_at_len_returns_none_and_leaves_self_unchanged() {
    let mut m = nebm(b"hello");
    assert!(m.split_to(nz(5)).is_none());
    assert_eq!(&m[..], b"hello");
}

#[test]
fn split_to_past_len_returns_none_and_leaves_self_unchanged() {
    let mut m = nebm(b"hello");
    assert!(m.split_to(nz(6)).is_none());
    assert!(m.split_to(nz(usize::MAX)).is_none());
    assert_eq!(&m[..], b"hello");
}

#[test]
fn split_to_of_single_byte_always_returns_none() {
    let mut m = nebm(b"a");
    assert!(m.split_to(nz(1)).is_none());
    assert_eq!(&m[..], b"a");
}

#[test]
fn split_to_exhaustive_small_cases() {
    let data: Vec<u8> = (0..8).collect();
    for len in 1..=data.len() {
        for at in 1..=data.len() + 2 {
            let mut m = nebm(&data[..len]);
            let result = m.split_to(nz(at));
            if at < len {
                let head = result.expect("at < len must succeed");
                assert_eq!(&head[..], &data[..at], "len={len} at={at}");
                assert_eq!(&m[..], &data[at..len], "len={len} at={at}");
            } else {
                assert!(result.is_none(), "len={len} at={at}");
                assert_eq!(&m[..], &data[..len], "len={len} at={at}");
            }
        }
    }
}

// --------------------------------------------------------------- split_off

#[test]
fn split_off_splits_into_two_non_empty_halves() {
    let mut m = nebm(b"hello world");
    let tail = m.split_off(nz(5)).unwrap();
    assert_eq!(&m[..], b"hello");
    assert_eq!(&tail[..], b" world");
}

#[test]
fn split_off_halves_are_independently_mutable() {
    let mut m = nebm(b"hello world");
    let mut tail = m.split_off(nz(5)).unwrap();

    m[0] = b'j';
    tail[0] = b'!';

    assert_eq!(&m[..], b"jello");
    assert_eq!(&tail[..], b"!world");
}

#[test]
fn split_off_at_one_leaves_a_single_byte_in_self() {
    let mut m = nebm(b"abc");
    let tail = m.split_off(nz(1)).unwrap();
    assert_eq!(&m[..], b"a");
    assert_eq!(&tail[..], b"bc");
}

#[test]
fn split_off_at_len_minus_one_leaves_a_single_byte_in_tail() {
    let mut m = nebm(b"abc");
    let tail = m.split_off(nz(2)).unwrap();
    assert_eq!(&m[..], b"ab");
    assert_eq!(&tail[..], b"c");
}

#[test]
fn split_off_at_len_returns_none_and_leaves_self_unchanged() {
    let mut m = nebm(b"hello");
    assert!(m.split_off(nz(5)).is_none());
    assert_eq!(&m[..], b"hello");
}

#[test]
fn split_off_past_len_returns_none_and_leaves_self_unchanged() {
    let mut m = nebm(b"hello");
    assert!(m.split_off(nz(6)).is_none());
    assert!(m.split_off(nz(usize::MAX)).is_none());
    assert_eq!(&m[..], b"hello");
}

#[test]
fn split_off_of_single_byte_always_returns_none() {
    let mut m = nebm(b"a");
    assert!(m.split_off(nz(1)).is_none());
    assert_eq!(&m[..], b"a");
}

#[test]
fn split_off_does_not_move_memory() {
    let mut m = nebm(b"hello world");
    let base = m.as_ptr();
    let tail = m.split_off(nz(5)).unwrap();
    assert_eq!(m.as_ptr(), base);
    assert_eq!(tail.as_ptr(), base.wrapping_add(5));
}

#[test]
fn split_off_exhaustive_small_cases() {
    let data: Vec<u8> = (0..8).collect();
    for len in 1..=data.len() {
        for at in 1..=data.len() + 2 {
            let mut m = nebm(&data[..len]);
            let result = m.split_off(nz(at));
            if at < len {
                let tail = result.expect("at < len must succeed");
                assert_eq!(&m[..], &data[..at], "len={len} at={at}");
                assert_eq!(&tail[..], &data[at..len], "len={len} at={at}");
            } else {
                assert!(result.is_none(), "len={len} at={at}");
                assert_eq!(&m[..], &data[..len], "len={len} at={at}");
            }
        }
    }
}

// ------------------------------------------------------ unsplit / try_unsplit

#[test]
fn unsplit_rejoins_contiguous_halves() {
    let mut m = nebm(b"aaabbbcccddd");
    let tail = m.split_off(nz(6)).unwrap();
    assert_eq!(&m[..], b"aaabbb");
    assert_eq!(&tail[..], b"cccddd");

    m.unsplit(tail.into_inner());
    assert_eq!(&m[..], b"aaabbbcccddd");
}

#[test]
fn unsplit_appends_a_separately_allocated_buffer() {
    let mut m = nebm(b"abc");
    m.unsplit(BytesMut::from(&b"def"[..]));
    assert_eq!(&m[..], b"abcdef");
}

#[test]
fn unsplit_with_an_empty_buffer_keeps_self_unchanged() {
    let mut m = nebm(b"abc");
    m.unsplit(BytesMut::new());
    assert_eq!(&m[..], b"abc");
    assert_eq!(m.len(), nz(3));
}

#[test]
fn try_unsplit_succeeds_for_contiguous_halves() {
    let mut m = nebm(b"aaabbb");
    let tail = m.split_off(nz(3)).unwrap();
    assert!(m.try_unsplit(tail.into_inner()).is_ok());
    assert_eq!(&m[..], b"aaabbb");
}

#[test]
fn try_unsplit_returns_the_buffer_when_not_contiguous() {
    let mut m = nebm(b"abc");
    let other = BytesMut::from(&b"zzz"[..]);

    let returned = m
        .try_unsplit(other)
        .expect_err("separately allocated buffers cannot be rejoined");

    assert_eq!(&returned[..], b"zzz");
    assert_eq!(&m[..], b"abc", "self must be unchanged on failure");
}

#[test]
fn try_unsplit_with_an_empty_buffer_is_ok_and_keeps_self_unchanged() {
    let mut m = nebm(b"abc");
    assert!(m.try_unsplit(BytesMut::new()).is_ok());
    assert_eq!(&m[..], b"abc");
}

#[test]
fn try_unsplit_only_accepts_the_adjacent_piece() {
    let mut buf = nebm(b"aaabbbcccddd");

    let mut split_1 = buf.split_off(nz(3)).unwrap();
    let split_2 = split_1.split_off(nz(3)).unwrap();
    assert_eq!(&buf[..], b"aaa");
    assert_eq!(&split_1[..], b"bbb");
    assert_eq!(&split_2[..], b"cccddd");

    // `split_2` is not adjacent to `buf` yet.
    let split_2 = buf.try_unsplit(split_2.into_inner()).unwrap_err();

    buf.try_unsplit(split_1.into_inner()).unwrap();
    buf.try_unsplit(split_2).unwrap();
    assert_eq!(&buf[..], b"aaabbbcccddd");
}

// ----------------------------------------------------------------- reserve

#[test]
fn reserve_increases_capacity_and_preserves_contents() {
    let mut m = nebm(b"hello");
    m.reserve(64);
    assert!(m.capacity().get() >= 69);
    assert_eq!(&m[..], b"hello");
    assert_eq!(m.len(), nz(5));
}

#[test]
fn reserve_zero_is_a_no_op() {
    let mut m = nebm(b"hello");
    m.reserve(0);
    assert_eq!(&m[..], b"hello");
}

#[test]
fn reserve_reclaims_the_original_allocation_once_other_handles_are_dropped() {
    let mut m = nebm(&[0u8; 64]);
    m.reserve(64);

    let capacity = m.capacity().get();
    let ptr = m.as_ptr();

    let head = m.split_to(nz(32)).unwrap();
    assert_eq!(m.capacity().get(), capacity - 32);

    drop(head);
    m.reserve(capacity - 32);

    assert_eq!(m.capacity().get(), capacity);
    assert_eq!(m.as_ptr(), ptr);
}

// ------------------------------------------------------------- try_reclaim

#[test]
fn try_reclaim_succeeds_when_spare_capacity_is_sufficient() {
    let mut m = nebm(b"abcd");
    m.reserve(64);
    assert!(m.try_reclaim(60));
}

#[test]
fn try_reclaim_fails_when_the_allocation_is_too_small() {
    let mut m = nebm(b"abcd");
    m.reserve(64);
    assert!(!m.try_reclaim(1 << 20));
}

#[test]
fn try_reclaim_fails_while_shared_and_succeeds_once_unique() {
    let mut m = nebm(b"abcd");
    m.reserve(64);

    let tail = m.split_off(nz(2)).unwrap();
    assert!(!m.try_reclaim(60), "tail still shares the allocation");

    drop(tail);
    assert!(m.try_reclaim(60), "allocation is reclaimable once unique");
    assert_eq!(&m[..], b"ab");
}

// ----------------------------------------------------------------- extend

#[test]
fn extend_from_slice_appends_bytes() {
    let mut m = nebm(b"aaabbb");
    m.extend_from_slice(b"cccddd");
    assert_eq!(&m[..], b"aaabbbcccddd");
}

#[test]
fn extend_from_slice_with_empty_slice_is_a_no_op() {
    let mut m = nebm(b"abc");
    m.extend_from_slice(&[]);
    assert_eq!(&m[..], b"abc");
    assert_eq!(m.len(), nz(3));
}

#[test]
fn extend_from_within_appends_a_copy_of_the_range() {
    let mut m = nebm(b"aaabbb_");
    m.extend_from_within(3..6);
    assert_eq!(&m[..], b"aaabbb_bbb");
}

#[test]
fn extend_from_within_supports_all_range_kinds() {
    let mut m = nebm(b"abc");
    m.extend_from_within(..);
    assert_eq!(&m[..], b"abcabc");

    let mut m = nebm(b"abc");
    m.extend_from_within(1..);
    assert_eq!(&m[..], b"abcbc");

    let mut m = nebm(b"abc");
    m.extend_from_within(..=0);
    assert_eq!(&m[..], b"abca");
}

#[test]
fn extend_from_within_with_empty_range_is_a_no_op() {
    let mut m = nebm(b"abc");
    m.extend_from_within(1..1);
    assert_eq!(&m[..], b"abc");
}

#[test]
#[should_panic]
fn extend_from_within_panics_when_range_is_out_of_bounds() {
    let mut m = nebm(b"abc");
    m.extend_from_within(2..10);
}

#[test]
fn extend_trait_accepts_owned_bytes() {
    let mut m = nebm(b"a");
    m.extend(vec![b'b', b'c']);
    assert_eq!(&m[..], b"abc");
}

#[test]
fn extend_trait_accepts_borrowed_bytes() {
    let mut m = nebm(b"a");
    m.extend([b'b', b'c'].iter());
    assert_eq!(&m[..], b"abc");
}

#[test]
fn extend_trait_accepts_bytes_chunks() {
    let mut m = nebm(b"a");
    m.extend(vec![Bytes::from_static(b"bc"), Bytes::from_static(b"de")]);
    assert_eq!(&m[..], b"abcde");
}

#[test]
fn extend_trait_with_empty_iterators_is_a_no_op() {
    let mut m = nebm(b"abc");
    m.extend(Vec::<u8>::new());
    m.extend(Vec::<Bytes>::new());
    assert_eq!(&m[..], b"abc");
    assert_eq!(m.len(), nz(3));
}

#[test]
fn fmt_write_appends_formatted_text() {
    let mut m = nebm(b"hello");
    m.write_str(" world").unwrap();
    write!(m, " {}", 42).unwrap();
    assert_eq!(&m[..], b"hello world 42");
}

// ------------------------------------------------------ NonEmptyBytes glue

#[test]
fn from_non_empty_bytes_when_unique() {
    let frozen = NonEmptyBytes::try_from(b"hello".to_vec()).unwrap();
    let m = NonEmptyBytesMut::from(frozen);
    assert_eq!(&m[..], b"hello");
}

#[test]
fn from_non_empty_bytes_when_shared_copies_and_isolates_mutation() {
    let original = NonEmptyBytes::try_from(b"hello".to_vec()).unwrap();
    let keep = original.clone();

    let mut m = NonEmptyBytesMut::from(original);
    m[0] = b'j';

    assert_eq!(&m[..], b"jello");
    assert_eq!(&keep[..], b"hello", "the shared original must be untouched");
}

#[test]
fn from_non_empty_bytes_backed_by_static_data_copies() {
    let frozen = NonEmptyBytes::from_static(b"hello").unwrap();
    let mut m = NonEmptyBytesMut::from(frozen);
    m[0] = b'j';
    assert_eq!(&m[..], b"jello");
}

#[test]
fn from_non_empty_bytes_backed_by_owner_copies() {
    let frozen = NonEmptyBytes::from_owner(b"hello".to_vec()).unwrap();
    let mut m = NonEmptyBytesMut::from(frozen);
    m[0] = b'j';
    assert_eq!(&m[..], b"jello");
}

#[test]
fn into_non_empty_bytes_via_from_equals_freeze() {
    let frozen = NonEmptyBytes::from(nebm(b"hello"));
    assert_eq!(&frozen[..], b"hello");
}

#[test]
fn round_trip_through_frozen_and_back() {
    let mut m = nebm(b"hello");
    m.extend_from_slice(b" world");
    let back = NonEmptyBytesMut::from(m.freeze());
    assert_eq!(&back[..], b"hello world");
}

// ------------------------------------------------------------ trait impls

#[test]
fn as_ref_and_deref_expose_the_bytes() {
    let m = nebm(b"abc");
    assert_eq!(AsRef::<[u8]>::as_ref(&m), b"abc");
    let derefed: &[u8] = &m;
    assert_eq!(derefed, b"abc");
}

#[test]
fn as_mut_and_deref_mut_allow_in_place_mutation_without_changing_len() {
    let mut m = nebm(b"abc");

    AsMut::<[u8]>::as_mut(&mut m)[0] = b'x';
    assert_eq!(&m[..], b"xbc");

    m[1] = b'y';
    assert_eq!(&m[..], b"xyc");

    m.reverse();
    assert_eq!(&m[..], b"cyx");

    m.fill(b'z');
    assert_eq!(&m[..], b"zzz");
    assert_eq!(m.len(), nz(3));
}

#[test]
fn borrow_and_borrow_mut_expose_the_bytes() {
    let mut m = nebm(b"abc");

    let borrowed: &[u8] = <NonEmptyBytesMut as Borrow<[u8]>>::borrow(&m);
    assert_eq!(borrowed, b"abc");

    <NonEmptyBytesMut as BorrowMut<[u8]>>::borrow_mut(&mut m)[0] = b'x';
    assert_eq!(&m[..], b"xbc");
}

#[test]
fn into_iterator_by_value_yields_every_byte_in_order() {
    let collected: Vec<u8> = nebm(&[1u8, 2, 3]).into_iter().collect();
    assert_eq!(collected, vec![1u8, 2, 3]);
}

#[test]
fn into_iterator_by_reference_yields_every_byte_in_order() {
    let m = nebm(&[1u8, 2, 3]);
    let mut seen: Vec<u8> = Vec::new();
    for byte in &m {
        seen.push(*byte);
    }
    assert_eq!(seen, vec![1u8, 2, 3]);
    assert_eq!(&m[..], &[1u8, 2, 3]);
}

#[test]
fn into_iterator_by_mutable_reference_allows_in_place_edits() {
    let mut m = nebm(&[1u8, 2, 3]);
    for byte in &mut m {
        *byte += 10;
    }
    assert_eq!(&m[..], &[11u8, 12, 13]);
    assert_eq!(m.len(), nz(3));
}

#[test]
fn clone_is_equal_and_independent() {
    let original = nebm(b"hello");
    let mut copy = original.clone();
    assert_eq!(original, copy);

    copy[0] = b'j';
    assert_eq!(&original[..], b"hello");
    assert_eq!(&copy[..], b"jello");
    assert_ne!(original, copy);
}

#[test]
fn hash_is_consistent_with_eq() {
    let mut set = HashSet::new();
    assert!(set.insert(nebm(b"abc")));
    assert!(
        !set.insert(nebm(b"abc")),
        "equal values must hash identically"
    );
    assert!(set.insert(nebm(b"abd")));
    assert_eq!(set.len(), 2);
    assert!(set.contains(&nebm(b"abc")));
}

#[test]
fn debug_output_names_the_type() {
    let rendered = format!("{:?}", nebm(b"a"));
    assert!(rendered.starts_with("NonEmptyBytesMut("), "got: {rendered}");
}

#[test]
fn is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<NonEmptyBytesMut>();
}

// ------------------------------------------------------------ conversions

#[test]
fn converts_into_bytes_mut() {
    let converted = BytesMut::from(nebm(b"hello"));
    assert_eq!(&converted[..], b"hello");
}

#[test]
fn converts_into_bytes() {
    let converted = Bytes::from(nebm(b"hello"));
    assert_eq!(&converted[..], b"hello");
}

#[test]
fn converts_into_vec() {
    let converted = Vec::<u8>::from(nebm(b"hello"));
    assert_eq!(converted, b"hello".to_vec());
}

// --------------------------------------------------------------- TryFrom

#[test]
fn try_from_accepts_non_empty_values_of_every_supported_type() {
    let from_bytes_mut = NonEmptyBytesMut::try_from(BytesMut::from(&b"ab"[..])).unwrap();
    assert_eq!(&from_bytes_mut[..], b"ab");

    let from_slice = NonEmptyBytesMut::try_from(&b"ab"[..]).unwrap();
    assert_eq!(&from_slice[..], b"ab");

    let from_str = NonEmptyBytesMut::try_from("ab").unwrap();
    assert_eq!(&from_str[..], b"ab");

    let from_bytes = NonEmptyBytesMut::try_from(Bytes::from_static(b"ab")).unwrap();
    assert_eq!(&from_bytes[..], b"ab");

    let from_vec = NonEmptyBytesMut::try_from(vec![b'a', b'b']).unwrap();
    assert_eq!(&from_vec[..], b"ab");

    let from_box = NonEmptyBytesMut::try_from(vec![b'a', b'b'].into_boxed_slice()).unwrap();
    assert_eq!(&from_box[..], b"ab");

    let from_string = NonEmptyBytesMut::try_from(String::from("ab")).unwrap();
    assert_eq!(&from_string[..], b"ab");
}

#[test]
fn try_from_rejects_empty_values_of_every_supported_type() {
    let empty_slice: &[u8] = &[];
    let empty_str: &str = "";

    assert!(matches!(
        NonEmptyBytesMut::try_from(BytesMut::new()),
        Err(EmptyBytesError)
    ));
    assert!(matches!(
        NonEmptyBytesMut::try_from(empty_slice),
        Err(EmptyBytesError)
    ));
    assert!(matches!(
        NonEmptyBytesMut::try_from(empty_str),
        Err(EmptyBytesError)
    ));
    assert!(matches!(
        NonEmptyBytesMut::try_from(Bytes::new()),
        Err(EmptyBytesError)
    ));
    assert!(matches!(
        NonEmptyBytesMut::try_from(Vec::<u8>::new()),
        Err(EmptyBytesError)
    ));
    assert!(matches!(
        NonEmptyBytesMut::try_from(Vec::<u8>::new().into_boxed_slice()),
        Err(EmptyBytesError)
    ));
    assert!(matches!(
        NonEmptyBytesMut::try_from(String::new()),
        Err(EmptyBytesError)
    ));
}

#[test]
fn try_from_vec_result_is_mutable() {
    let mut m = NonEmptyBytesMut::try_from(vec![1u8, 2, 3]).unwrap();
    m[0] = 9;
    m.extend_from_slice(&[4]);
    assert_eq!(&m[..], &[9u8, 2, 3, 4]);
}

// ----------------------------------------------------------- comparisons

#[test]
fn eq_with_every_supported_type_in_both_directions() {
    let m = nebm(b"abc");

    assert!(m == BytesMut::from(&b"abc"[..]));
    assert!(BytesMut::from(&b"abc"[..]) == m);

    assert!(m == Bytes::from_static(b"abc"));
    assert!(Bytes::from_static(b"abc") == m);

    assert!(m == b"abc"[..]);
    assert!(b"abc"[..] == m);

    assert!(m == &b"abc"[..]);
    assert!(&b"abc"[..] == m);

    assert!(m == *"abc");
    assert!(*"abc" == m);

    assert!(m == "abc");
    assert!("abc" == m);

    assert!(m == b"abc".to_vec());
    assert!(b"abc".to_vec() == m);

    assert!(m == String::from("abc"));
    assert!(String::from("abc") == m);
}

#[test]
fn ne_with_every_supported_type_in_both_directions() {
    let m = nebm(b"abc");

    assert!(m != BytesMut::from(&b"abd"[..]));
    assert!(BytesMut::from(&b"abd"[..]) != m);

    assert!(m != Bytes::from_static(b"abd"));
    assert!(Bytes::from_static(b"abd") != m);

    assert!(m != b"abd"[..]);
    assert!(b"abd"[..] != m);

    assert!(m != &b"abd"[..]);
    assert!(&b"abd"[..] != m);

    assert!(m != *"abd");
    assert!(*"abd" != m);

    assert!(m != "abd");
    assert!("abd" != m);

    assert!(m != b"abd".to_vec());
    assert!(b"abd".to_vec() != m);

    assert!(m != String::from("abd"));
    assert!(String::from("abd") != m);
}

#[test]
fn eq_distinguishes_different_lengths() {
    let m = nebm(b"abc");
    assert!(m != "ab");
    assert!(m != "abcd");
    assert!(m != "");
    assert!("" != m);
}

#[test]
fn partial_ord_with_every_supported_type_in_both_directions() {
    let m = nebm(b"abc");

    assert!(m < BytesMut::from(&b"abd"[..]));
    assert!(BytesMut::from(&b"abd"[..]) > m);

    assert!(m < Bytes::from_static(b"abd"));
    assert!(Bytes::from_static(b"abd") > m);

    assert!(m < b"abd"[..]);
    assert!(b"abd"[..] > m);

    assert!(m < &b"abd"[..]);
    assert!(&b"abd"[..] > m);

    assert!(m < *"abd");
    assert!(*"abd" > m);

    assert!(m < "abd");
    assert!("abd" > m);

    assert!(m < b"abd".to_vec());
    assert!(b"abd".to_vec() > m);

    assert!(m < String::from("abd"));
    assert!(String::from("abd") > m);
}

#[test]
fn partial_cmp_reports_equal_less_and_greater() {
    use std::cmp::Ordering;
    let m = nebm(b"abc");
    assert_eq!(
        m.partial_cmp(&BytesMut::from(&b"abc"[..])),
        Some(Ordering::Equal)
    );
    assert_eq!(m.partial_cmp(&"abc"), Some(Ordering::Equal));
    assert_eq!(m.partial_cmp(&b"abd".to_vec()), Some(Ordering::Less));
    assert_eq!(m.partial_cmp(&String::from("abb")), Some(Ordering::Greater));
}

#[test]
fn ordering_is_lexicographic_and_prefix_is_smaller() {
    assert!(nebm(b"ab") < nebm(b"abc"));
    assert!(nebm(b"abc") < nebm(b"abd"));
    assert!(nebm(b"b") > nebm(b"abc"));
    assert_eq!(nebm(b"abc").cmp(&nebm(b"abc")), std::cmp::Ordering::Equal);
}

#[test]
fn sorting_uses_byte_order() {
    let mut values = vec![nebm(b"b"), nebm(b"abc"), nebm(b"ab"), nebm(b"a")];
    values.sort();
    let sorted: Vec<&[u8]> = values.iter().map(|v| &v[..]).collect();
    assert_eq!(sorted, vec![&b"a"[..], &b"ab"[..], &b"abc"[..], &b"b"[..]]);
}

// -------------------------------------------------------- invariant sweep

/// Drives a buffer through every shrinking operation the API offers and
/// checks that `len()` never reaches zero.
#[test]
fn shrinking_operations_never_produce_an_empty_buffer() {
    let mut m = nebm(b"0123456789");

    m.truncate(nz(8));
    assert!(!m.is_empty());

    let _tail = m.split_off(nz(6)).unwrap();
    assert!(!m.is_empty());

    let _head = m.split_to(nz(2)).unwrap();
    assert!(!m.is_empty());

    m.resize(nz(1), 0);
    assert_eq!(m.len(), nz(1));

    // At length one, every split must refuse.
    assert!(m.split_to(nz(1)).is_none());
    assert!(m.split_off(nz(1)).is_none());
    m.truncate(nz(1));
    assert_eq!(m.len(), nz(1));
}
