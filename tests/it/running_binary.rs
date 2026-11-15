use guided_review::platform::running_binary_from;
use std::path::PathBuf;

#[test]
fn a_binary_replaced_while_it_runs_is_named_by_the_path_of_its_replacement() {
    let reported = PathBuf::from("/home/me/.cargo/bin/guided-review (deleted)");

    assert_eq!(
        running_binary_from(reported),
        PathBuf::from("/home/me/.cargo/bin/guided-review")
    );
}

#[test]
fn a_binary_still_in_place_keeps_its_path() {
    let reported = PathBuf::from("/home/me/.local/bin/guided-review");

    assert_eq!(running_binary_from(reported.clone()), reported);
}
