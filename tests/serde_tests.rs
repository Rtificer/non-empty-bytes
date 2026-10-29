//! Tests for the optional `serde` support.
//!
//! These only exercise what this crate adds on top of `serde` and `bytes`:
//! that both wrappers serialize as a byte string, and that deserialization
//! rejects empty input with the crate's own error. How `bytes::Bytes` and
//! `bytes::BytesMut` themselves (de)serialize is assumed to be correct.
//!
//! Run with: `cargo test --features serde`

#![cfg(feature = "serde")]

use non_empty_bytes::{NonEmptyBytes, NonEmptyBytesMut};
use serde_test::{
    Token, assert_de_tokens, assert_de_tokens_error, assert_ser_tokens, assert_tokens,
};

const EMPTY_ERROR: &str = "invalid length 0, expected at least one byte";

fn neb(data: &[u8]) -> NonEmptyBytes {
    NonEmptyBytes::copy_from_slice(data).expect("test helper requires non-empty data")
}

fn nebm(data: &[u8]) -> NonEmptyBytesMut {
    NonEmptyBytesMut::copy_from_slice(data).expect("test helper requires non-empty data")
}

// ------------------------------------------------------- NonEmptyBytes: ser

#[test]
fn bytes_serializes_as_a_byte_string() {
    assert_ser_tokens(&neb(b"abc"), &[Token::Bytes(b"abc")]);
}

#[test]
fn bytes_serializes_a_single_byte() {
    assert_ser_tokens(&neb(b"z"), &[Token::Bytes(b"z")]);
}

#[test]
fn bytes_serializes_only_the_visible_slice() {
    let view = neb(b"hello world").slice(6..).unwrap();
    assert_ser_tokens(&view, &[Token::Bytes(b"world")]);
}

#[test]
fn bytes_serializes_non_utf8_data() {
    assert_ser_tokens(
        &neb(&[0x00, 0xff, 0xfe]),
        &[Token::Bytes(&[0x00, 0xff, 0xfe])],
    );
}

// ----------------------------------------------------- NonEmptyBytes: de

#[test]
fn bytes_deserializes_from_every_byte_string_form() {
    let expected = neb(b"abc");
    assert_de_tokens(&expected, &[Token::Bytes(b"abc")]);
    assert_de_tokens(&expected, &[Token::BorrowedBytes(b"abc")]);
    assert_de_tokens(&expected, &[Token::ByteBuf(b"abc")]);
}

#[test]
fn bytes_deserializes_from_a_sequence_of_integers() {
    assert_de_tokens(
        &neb(&[1, 2, 3]),
        &[
            Token::Seq { len: Some(3) },
            Token::U8(1),
            Token::U8(2),
            Token::U8(3),
            Token::SeqEnd,
        ],
    );
}

#[test]
fn bytes_deserializes_a_single_byte() {
    assert_de_tokens(&neb(b"z"), &[Token::Bytes(b"z")]);
}

#[test]
fn bytes_rejects_empty_input_in_every_form() {
    assert_de_tokens_error::<NonEmptyBytes>(&[Token::Bytes(b"")], EMPTY_ERROR);
    assert_de_tokens_error::<NonEmptyBytes>(&[Token::BorrowedBytes(b"")], EMPTY_ERROR);
    assert_de_tokens_error::<NonEmptyBytes>(&[Token::ByteBuf(b"")], EMPTY_ERROR);
    assert_de_tokens_error::<NonEmptyBytes>(
        &[Token::Seq { len: Some(0) }, Token::SeqEnd],
        EMPTY_ERROR,
    );
}

#[test]
fn bytes_round_trips() {
    assert_tokens(&neb(b"hello"), &[Token::Bytes(b"hello")]);
}

// -------------------------------------------------- NonEmptyBytesMut: ser

#[test]
fn bytes_mut_serializes_as_a_byte_string() {
    assert_ser_tokens(&nebm(b"abc"), &[Token::Bytes(b"abc")]);
}

#[test]
fn bytes_mut_serializes_a_single_byte() {
    assert_ser_tokens(&nebm(b"z"), &[Token::Bytes(b"z")]);
}

#[test]
fn bytes_mut_serializes_current_contents_after_mutation() {
    let mut m = nebm(b"abc");
    m.extend_from_slice(b"def");
    m[0] = b'X';
    assert_ser_tokens(&m, &[Token::Bytes(b"Xbcdef")]);
}

// ---------------------------------------------------- NonEmptyBytesMut: de

#[test]
fn bytes_mut_deserializes_from_every_byte_string_form() {
    let expected = nebm(b"abc");
    assert_de_tokens(&expected, &[Token::Bytes(b"abc")]);
    assert_de_tokens(&expected, &[Token::BorrowedBytes(b"abc")]);
    assert_de_tokens(&expected, &[Token::ByteBuf(b"abc")]);
}

#[test]
fn bytes_mut_deserializes_from_a_sequence_of_integers() {
    assert_de_tokens(
        &nebm(&[1, 2, 3]),
        &[
            Token::Seq { len: Some(3) },
            Token::U8(1),
            Token::U8(2),
            Token::U8(3),
            Token::SeqEnd,
        ],
    );
}

#[test]
fn bytes_mut_rejects_empty_input_in_every_form() {
    assert_de_tokens_error::<NonEmptyBytesMut>(&[Token::Bytes(b"")], EMPTY_ERROR);
    assert_de_tokens_error::<NonEmptyBytesMut>(&[Token::BorrowedBytes(b"")], EMPTY_ERROR);
    assert_de_tokens_error::<NonEmptyBytesMut>(&[Token::ByteBuf(b"")], EMPTY_ERROR);
    assert_de_tokens_error::<NonEmptyBytesMut>(
        &[Token::Seq { len: Some(0) }, Token::SeqEnd],
        EMPTY_ERROR,
    );
}

#[test]
fn bytes_mut_round_trips() {
    assert_tokens(&nebm(b"hello"), &[Token::Bytes(b"hello")]);
}

// ------------------------------------------------------------ via serde_json

#[test]
fn json_round_trip_for_both_types() {
    let json = serde_json::to_string(&neb(&[104, 105])).unwrap();
    assert_eq!(json, "[104,105]");
    let back: NonEmptyBytes = serde_json::from_str(&json).unwrap();
    assert_eq!(back, neb(b"hi"));

    let json = serde_json::to_string(&nebm(&[104, 105])).unwrap();
    assert_eq!(json, "[104,105]");
    let back: NonEmptyBytesMut = serde_json::from_str(&json).unwrap();
    assert_eq!(back, nebm(b"hi"));
}

#[test]
fn both_types_serialize_identically() {
    let a = serde_json::to_string(&neb(b"payload")).unwrap();
    let b = serde_json::to_string(&nebm(b"payload")).unwrap();
    assert_eq!(a, b);
}

#[test]
fn either_type_can_be_read_from_the_other_types_output() {
    let json = serde_json::to_string(&neb(b"abc")).unwrap();
    let as_mut: NonEmptyBytesMut = serde_json::from_str(&json).unwrap();
    assert_eq!(as_mut, "abc");

    let json = serde_json::to_string(&nebm(b"abc")).unwrap();
    let as_bytes: NonEmptyBytes = serde_json::from_str(&json).unwrap();
    assert_eq!(as_bytes, "abc");
}

#[test]
fn json_empty_array_is_rejected_with_the_crates_error() {
    let err = serde_json::from_str::<NonEmptyBytes>("[]").unwrap_err();
    assert!(err.to_string().contains(EMPTY_ERROR), "got: {err}");

    let err = serde_json::from_str::<NonEmptyBytesMut>("[]").unwrap_err();
    assert!(err.to_string().contains(EMPTY_ERROR), "got: {err}");
}

#[test]
fn json_empty_string_is_rejected() {
    assert!(serde_json::from_str::<NonEmptyBytes>("\"\"").is_err());
    assert!(serde_json::from_str::<NonEmptyBytesMut>("\"\"").is_err());
}

#[test]
fn rejection_propagates_out_of_containers() {
    assert!(serde_json::from_str::<Vec<NonEmptyBytes>>("[[1],[2,3]]").is_ok());
    assert!(serde_json::from_str::<Vec<NonEmptyBytes>>("[[1],[]]").is_err());
    assert!(serde_json::from_str::<Vec<NonEmptyBytesMut>>("[[1],[]]").is_err());
}

#[test]
fn option_distinguishes_null_from_empty() {
    let none: Option<NonEmptyBytes> = serde_json::from_str("null").unwrap();
    assert!(none.is_none());

    let some: Option<NonEmptyBytes> = serde_json::from_str("[1]").unwrap();
    assert_eq!(some.unwrap(), neb(&[1]));

    assert!(serde_json::from_str::<Option<NonEmptyBytes>>("[]").is_err());
}

#[test]
fn deserialized_values_are_fully_usable() {
    let x: NonEmptyBytes = serde_json::from_str("[1,2,3]").unwrap();
    assert_eq!(x.len().get(), 3);
    assert_eq!(x.slice(1..).unwrap(), &[2u8, 3][..]);

    let mut m: NonEmptyBytesMut = serde_json::from_str("[1,2,3]").unwrap();
    m.extend_from_slice(&[4]);
    assert_eq!(m.len().get(), 4);
    assert_eq!(m.freeze(), &[1u8, 2, 3, 4][..]);
}
