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
