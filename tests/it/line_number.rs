use guided_review::line_number::LineNumber;
use serde_json::json;

#[test]
fn lines_count_from_one_and_fit_32_bits() {
    assert!(LineNumber::try_from(0_usize).is_err());
    assert_eq!(LineNumber::try_from(1_usize).unwrap(), LineNumber::FIRST);
    assert_eq!(LineNumber::try_from(40_usize).unwrap().get(), 40);
    assert!(LineNumber::try_from(usize::MAX).is_err());
}

#[test]
fn a_line_is_a_plain_number_in_a_state_file_and_zero_is_refused() {
    let line = LineNumber::try_from(12_usize).unwrap();

    assert_eq!(serde_json::to_value(line).unwrap(), json!(12));
    assert_eq!(
        serde_json::from_value::<LineNumber>(json!(12)).unwrap(),
        line
    );
    assert!(serde_json::from_value::<LineNumber>(json!(0)).is_err());
}

#[test]
fn a_line_is_four_bytes_even_inside_an_option() {
    assert_eq!(size_of::<LineNumber>(), 4);
    assert_eq!(size_of::<Option<LineNumber>>(), 4);
}

#[test]
fn a_diff_line_stays_small_since_there_is_one_per_shown_line() {
    assert_eq!(size_of::<guided_review::viewer::DiffLine>(), 184);
}
