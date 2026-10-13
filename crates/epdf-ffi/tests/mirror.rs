use epdf_ffi::{mirror_enum, to_u32};

mod core_side {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Side {
        Left,
        Right,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Side {
    Left,
    Right,
}

mirror_enum!(Side <=> core_side::Side { Left, Right });

#[test]
fn a_mirrored_enum_round_trips() {
    for side in [Side::Left, Side::Right] {
        assert_eq!(Side::from(core_side::Side::from(side)), side);
    }
}

#[test]
fn a_count_past_u32_is_invalid_state() {
    assert_eq!(to_u32(7, "a page index").unwrap(), 7);
    assert_eq!(to_u32(usize::MAX, "a page index").unwrap_err().code(), Some("invalid-state"));
}
