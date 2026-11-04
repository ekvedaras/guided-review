use guided_review::timestamp::UtcTimestamp;

#[test]
fn shows_the_epoch_seconds_as_a_file_name_safe_utc_date() {
    assert_eq!(
        UtcTimestamp::from_epoch_seconds(0).to_string(),
        "1970-01-01T00-00-00"
    );
    assert_eq!(
        UtcTimestamp::from_epoch_seconds(1_782_133_509).to_string(),
        "2026-06-22T13-05-09"
    );
}
