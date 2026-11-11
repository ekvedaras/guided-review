use guided_review::tour::{StepId, Tour};

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

    let tour = Tour::load(&path).unwrap();
    tour.save(&path).unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), TOUR_JSON);
}

#[test]
fn a_step_is_found_by_its_id_at_any_depth() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tour.json");
    std::fs::write(&path, TOUR_JSON).unwrap();
    let tour = Tour::load(&path).unwrap();

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
        Tour::files_for(home, &pr),
        Tour::path_for(home, &pr).with_extension("")
    );
    assert!(Tour::files_for(home, &pr).ends_with("acme/shop/7"));
}
