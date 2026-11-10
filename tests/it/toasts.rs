use std::time::{Duration, Instant};

use guided_review::viewer::{ToastTone, Toasts};

fn texts(toasts: &Toasts) -> Vec<&str> {
    toasts.shown().map(|toast| toast.text.as_str()).collect()
}

#[test]
fn a_toast_leaves_by_itself_after_a_few_seconds() {
    let start = Instant::now();
    let mut toasts = Toasts::default();
    toasts.post(ToastTone::Success, "Comment added to your review", start);

    assert!(!toasts.expire(start + Duration::from_secs(3)));
    assert_eq!(texts(&toasts), ["Comment added to your review"]);
    assert!(toasts.expire(start + Toasts::LIFETIME));
    assert_eq!(texts(&toasts), [] as [&str; 0]);
}

#[test]
fn the_pointer_on_the_stack_holds_its_toasts_until_it_leaves() {
    let start = Instant::now();
    let mut toasts = Toasts::default();
    toasts.post(ToastTone::Success, "Settings saved", start);

    toasts.pause(start + Duration::from_secs(1));
    assert!(!toasts.expire(start + Duration::from_secs(60)));
    toasts.resume(start + Duration::from_secs(60));
    assert!(!toasts.expire(start + Duration::from_secs(62)));
    assert!(toasts.expire(start + Duration::from_secs(63)));
}

#[test]
fn a_failure_stays_until_it_is_dismissed() {
    let start = Instant::now();
    let mut toasts = Toasts::default();
    let failure = toasts.post(ToastTone::Danger, "Could not save", start);

    assert!(!toasts.expire(start + Duration::from_secs(600)));
    assert_eq!(texts(&toasts), ["Could not save"]);
    toasts.dismiss(failure);
    assert_eq!(texts(&toasts), [] as [&str; 0]);
}

#[test]
fn at_most_three_show_and_a_new_one_pushes_out_the_oldest_that_would_leave_anyway() {
    let start = Instant::now();
    let mut toasts = Toasts::default();
    toasts.post(ToastTone::Danger, "failed", start);
    toasts.post(ToastTone::Success, "one", start);
    toasts.post(ToastTone::Success, "two", start);
    toasts.post(ToastTone::Success, "three", start);

    assert_eq!(texts(&toasts), ["failed", "two", "three"]);
}

#[test]
fn the_next_expiry_says_when_to_look_again() {
    let start = Instant::now();
    let mut toasts = Toasts::default();
    assert_eq!(toasts.next_expiry(start), None);
    toasts.post(ToastTone::Success, "one", start);
    toasts.post(ToastTone::Danger, "stays", start);

    assert_eq!(
        toasts.next_expiry(start + Duration::from_secs(1)),
        Toasts::LIFETIME.checked_sub(Duration::from_secs(1))
    );
    toasts.pause(start + Duration::from_secs(1));
    assert_eq!(toasts.next_expiry(start + Duration::from_secs(2)), None);
}
