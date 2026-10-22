use guided_review::viewer::{Block, Markdown, Run};

fn plain(text: &str) -> Run {
    Run {
        text: text.into(),
        ..Run::default()
    }
}

fn paragraph_runs(block: &Block) -> &[Run] {
    match block {
        Block::Paragraph(runs) => runs,
        other => panic!("expected a paragraph, got {other:?}"),
    }
}

#[test]
fn bold_italic_code_and_links_keep_their_style_in_one_paragraph() {
    let markdown =
        Markdown::parse("a **bold** and *soft* with `code` and [docs](https://x.test/d)");

    let runs = paragraph_runs(&markdown.blocks[0]);
    assert_eq!(runs[0], plain("a "));
    assert!(runs[1].bold && runs[1].text == "bold");
    assert!(runs[3].italic && runs[3].text == "soft");
    assert!(runs[5].code && runs[5].text == "code");
    assert_eq!(runs[7].text, "docs");
    assert_eq!(runs[7].link.as_deref(), Some("https://x.test/d"));
}

#[test]
fn a_fenced_block_keeps_its_language_and_text() {
    let markdown = Markdown::parse("```php\n$a = 1;\n$b = 2;\n```");

    assert_eq!(
        markdown.blocks,
        [Block::Code {
            language: Some("php".into()),
            text: "$a = 1;\n$b = 2;\n".into()
        }]
    );
}

#[test]
fn lists_keep_their_items_and_numbering() {
    let markdown = Markdown::parse("- one\n- two\n\n3. three\n4. four");

    let Block::List {
        first_number,
        items,
    } = &markdown.blocks[0]
    else {
        panic!("{:?}", markdown.blocks)
    };
    assert_eq!((*first_number, items.len()), (None, 2));
    assert_eq!(paragraph_runs(&items[1][0]), [plain("two")]);
    let Block::List {
        first_number,
        items,
    } = &markdown.blocks[1]
    else {
        panic!("{:?}", markdown.blocks)
    };
    assert_eq!((*first_number, items.len()), (Some(3), 2));
}

#[test]
fn a_details_block_becomes_a_fold_with_a_summary_and_a_body() {
    let markdown = Markdown::parse(
        "before\n\n<details>\n<summary>Logs <b>here</b></summary>\n\nthe **body**\n\n```\ncode\n```\n\n</details>\n\nafter",
    );

    assert_eq!(markdown.blocks.len(), 3);
    let Block::Details { summary, body } = &markdown.blocks[1] else {
        panic!("{:?}", markdown.blocks)
    };
    assert_eq!(
        summary
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        "Logs here"
    );
    assert!(summary[1].bold);
    assert_eq!(body.len(), 2);
    assert!(matches!(body[1], Block::Code { .. }));
    assert_eq!(paragraph_runs(&markdown.blocks[2]), [plain("after")]);
}

#[test]
fn an_unclosed_details_still_ends_with_the_text_it_held() {
    let markdown = Markdown::parse("<details>\n<summary>S</summary>\n\ninside");

    let Block::Details { body, .. } = &markdown.blocks[0] else {
        panic!("{:?}", markdown.blocks)
    };
    assert_eq!(paragraph_runs(&body[0]), [plain("inside")]);
}

#[test]
fn images_become_links_to_the_image() {
    let markdown = Markdown::parse(
        "![the chart](https://img.test/a.png)\n\n<img width=\"10\" alt=\"shot\" src=\"https://img.test/b.png\">",
    );

    let first = &paragraph_runs(&markdown.blocks[0])[0];
    assert_eq!(
        (first.text.as_str(), first.link.as_deref()),
        ("the chart", Some("https://img.test/a.png"))
    );
    let second = &paragraph_runs(&markdown.blocks[1])[0];
    assert_eq!(
        (second.text.as_str(), second.link.as_deref()),
        ("shot", Some("https://img.test/b.png"))
    );
}

#[test]
fn html_comments_and_unknown_tags_leave_only_their_text() {
    let markdown = Markdown::parse("hi <!-- hidden --> <span>there</span>");

    let text: String = paragraph_runs(&markdown.blocks[0])
        .iter()
        .map(|run| run.text.as_str())
        .collect();
    assert_eq!(text, "hi there");
}
