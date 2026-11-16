use guided_review::tour::{BasicTour, GuidedTour, StepId, Tour};

const TOUR_JSON: &str = r#"{
  "version": 1,
  "pr": "https://github.com/acme/shop/pull/7",
  "base": "aaa",
  "head": "bbb",
  "previous_head": "ccc",
  "title": "Discounts",
  "summary": [
    "one"
  ],
  "steps": [
    {
      "id": "order",
      "title": "Order",
      "why": "because",
      "check": "look",
      "tldr": [
        "short"
      ],
      "diagram": "graph",
      "skim": true,
      "pieces": [
        {
          "rest": "app"
        },
        {
          "file": "a.txt",
          "lines": [
            1,
            4
          ]
        },
        {
          "file": "app/Order.php",
          "symbol": "Order::total"
        },
        {
          "file": "README.md"
        }
      ],
      "children": [
        {
          "id": "order-child",
          "title": "",
          "why": "",
          "skim": false
        }
      ]
    }
  ]
}
"#;

#[test]
fn a_tour_with_every_pointer_kind_is_written_back_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tour.json");
    std::fs::write(&path, TOUR_JSON).unwrap();

    let tour = GuidedTour::load(&path).unwrap();
    tour.save(&path).unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), TOUR_JSON);
}

#[test]
fn a_step_is_found_by_its_id_at_any_depth() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tour.json");
    std::fs::write(&path, TOUR_JSON).unwrap();
    let tour = GuidedTour::load(&path).unwrap();

    assert_eq!(
        tour.find_step(&StepId::from("order-child"))
            .map(|step| step.id.clone()),
        Some(StepId::from("order-child"))
    );
    assert!(tour.find_step(&StepId::from("nope")).is_none());
}

#[test]
fn a_symbol_or_whole_file_pointer_names_its_piece_and_the_other_kinds_name_none() {
    use guided_review::tour::PieceRef;

    let named = |pointer: PieceRef| pointer.piece_id().map(|id| id.to_string());

    assert_eq!(
        named(PieceRef::Symbol {
            file: "app/Order.php".into(),
            symbol: "Order::total".into()
        }),
        Some("app/Order.php#Order::total".to_owned())
    );
    assert_eq!(
        named(PieceRef::WholeFile {
            file: "logo.png".into()
        }),
        Some("logo.png".to_owned())
    );
    assert_eq!(
        named(PieceRef::Lines {
            file: "ci.yml".into(),
            lines: [1, 2]
        }),
        None
    );
    assert_eq!(named(PieceRef::Rest { rest: "app".into() }), None);
    assert_eq!(
        named(PieceRef::Symbol {
            file: "a.php".into(),
            symbol: "a/b".into()
        }),
        None
    );
}

#[test]
fn a_tours_own_files_sit_in_a_folder_beside_it_named_by_the_pr_number() {
    let pr = "https://github.com/acme/shop/pull/7".parse().unwrap();
    let home = std::path::Path::new("/home/me");

    assert_eq!(
        Tour::files_for(&crate::support::folders_in(home), &pr),
        Tour::path_for(&crate::support::folders_in(home), &pr).with_extension("")
    );
    assert!(Tour::files_for(&crate::support::folders_in(home), &pr).ends_with("acme/shop/7"));
}

fn tour_file(json: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tour.json");
    std::fs::write(&path, json).unwrap();
    (dir, path)
}

const PIN: &str = r#""pr": "https://github.com/acme/shop/pull/7", "base": "aaa", "head": "bbb""#;

#[test]
fn a_tour_file_with_no_kind_reads_as_a_guided_tour() {
    let (_dir, path) = tour_file(TOUR_JSON);

    assert!(matches!(Tour::load(&path).unwrap(), Tour::Guided(_)));
}

#[test]
fn a_tour_file_that_says_guided_reads_as_a_guided_tour() {
    let (_dir, path) = tour_file(&format!(r#"{{"kind": "guided", {PIN}, "title": "T"}}"#));

    let Tour::Guided(tour) = Tour::load(&path).unwrap() else {
        panic!("a file that says guided is a guided tour");
    };
    assert_eq!(tour.title, "T");
}

#[test]
fn a_basic_tour_file_reads_as_a_basic_tour_with_its_pin_and_the_prs_title() {
    let (_dir, path) = tour_file(&format!(
        r#"{{"kind": "basic", {PIN}, "previous_head": "ccc", "pr_title": "Discounts"}}"#
    ));

    let Tour::Basic(tour) = Tour::load(&path).unwrap() else {
        panic!("a file that says basic is a basic tour");
    };
    assert_eq!(tour.pr_title, "Discounts");
    assert_eq!(tour.pin.head.to_string(), "bbb");
    assert_eq!(tour.pin.base.to_string(), "aaa");
    assert_eq!(tour.pin.previous_head.unwrap().to_string(), "ccc");
}

#[test]
fn a_basic_tour_is_written_back_with_its_kind() {
    let (_dir, path) = tour_file(&format!(
        r#"{{"kind": "basic", {PIN}, "pr_title": "Discounts"}}"#
    ));

    let tour = BasicTour::load(&path).unwrap();
    tour.save(&path).unwrap();

    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(written["kind"], "basic");
    assert_eq!(written["pr_title"], "Discounts");
    assert_eq!(written["head"], "bbb");
}

#[test]
fn a_basic_tour_file_that_holds_what_only_a_guided_tour_has_is_refused() {
    for (field, value) in [
        ("title", r#""A title""#),
        ("summary", r#"["one"]"#),
        ("reviewer_summary", r#""Notes""#),
        ("steps", "[]"),
    ] {
        let (_dir, path) = tour_file(&format!(
            r#"{{"kind": "basic", {PIN}, "pr_title": "T", "{field}": {value}}}"#
        ));

        let refusal = format!("{:#}", Tour::load(&path).unwrap_err());

        assert!(refusal.contains(field), "{field}: {refusal}");
    }
}

#[test]
fn the_guided_path_refuses_a_basic_tour() {
    let (_dir, path) = tour_file(&format!(r#"{{"kind": "basic", {PIN}, "pr_title": "T"}}"#));

    assert!(GuidedTour::load(&path).is_err());
}

#[test]
fn a_basic_tour_lives_beside_the_guided_tours_file() {
    let pr = "https://github.com/acme/shop/pull/7".parse().unwrap();
    let folders = crate::support::folders_in(std::path::Path::new("/home/me"));

    let guided = Tour::path_for(&folders, &pr);
    let basic = Tour::basic_path_for(&folders, &pr);

    assert_eq!(guided.parent(), basic.parent());
    assert!(basic.ends_with("acme/shop/7.basic.json"));
}

mod pin_subject {
    use guided_review::pull_request::Sha;
    use guided_review::review_subject::ReviewSubject;
    use guided_review::tour::{GuidedTour, TourPin};

    use super::{TOUR_JSON, tour_file};

    const LOCAL_KEY: &str = "local:/Users/me/Code/shop@feature/discounts";
    const PR_URL: &str = "https://github.com/acme/shop/pull/7";

    fn pin(json: &str) -> TourPin {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_pull_requests_tour_file_pins_its_pull_request_and_saving_it_writes_no_local_key() {
        let (_dir, path) = tour_file(TOUR_JSON);
        let tour = GuidedTour::load(&path).unwrap();

        assert_eq!(
            tour.pin.subject().unwrap(),
            PR_URL.parse::<ReviewSubject>().unwrap()
        );
        tour.save(&path).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains(&format!("\"pr\": \"{PR_URL}\"")));
        assert!(!saved.contains("local"));
    }

    #[test]
    fn a_local_pin_round_trips_with_local_and_no_pr_url() {
        let mut pinned = pin("{}");
        let local: ReviewSubject = LOCAL_KEY.parse().unwrap();
        pinned.pin_to(&local, &Sha::new("aaa"), &Sha::new("bbb"));

        let json = serde_json::to_string(&pinned).unwrap();
        let read = pin(&json);

        assert!(!json.contains("github.com"));
        assert_eq!(read.local.as_deref(), Some(LOCAL_KEY));
        assert_eq!(read.subject().unwrap(), local);
    }

    #[test]
    fn pinning_a_pull_request_clears_a_local_key() {
        let mut pinned = pin(&format!(r#"{{"local": "{LOCAL_KEY}"}}"#));
        let pr: ReviewSubject = PR_URL.parse().unwrap();

        pinned.pin_to(&pr, &Sha::new("aaa"), &Sha::new("bbb"));

        assert_eq!(pinned.local, None);
        assert_eq!(pinned.subject().unwrap(), pr);
    }

    #[test]
    fn a_pin_naming_both_a_pull_request_and_a_local_change_set_is_refused() {
        let both = pin(&format!(r#"{{"pr": "{PR_URL}", "local": "{LOCAL_KEY}"}}"#));

        assert!(both.subject().is_err());
    }

    #[test]
    fn a_pin_naming_neither_is_refused_with_a_reason() {
        let error = pin("{}").subject().unwrap_err();

        assert!(error.to_string().contains("neither"), "{error}");
    }
}
