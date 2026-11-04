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

    let [
        Block::Code {
            language,
            text,
            spans,
        },
    ] = markdown.blocks.as_slice()
    else {
        panic!("one code block, got {:?}", markdown.blocks);
    };
    assert_eq!(language.as_deref(), Some("php"));
    assert_eq!(text, "$a = 1;\n$b = 2;\n");
    assert_eq!(spans.len(), 2, "one line of colours per line of code");
    assert!(spans.iter().all(|line| !line.is_empty()));
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

fn linked(markdown: &Markdown) -> Vec<(String, Option<String>)> {
    paragraph_runs(markdown.blocks.first().unwrap())
        .iter()
        .map(|run| (run.text.clone(), run.link.clone()))
        .collect()
}

fn link(text: &str) -> (String, Option<String>) {
    (text.to_owned(), Some(text.to_owned()))
}

fn unlinked(text: &str) -> (String, Option<String>) {
    (text.to_owned(), None)
}

#[test]
fn a_bare_url_is_a_link_without_the_punctuation_after_it() {
    let markdown =
        Markdown::parse("Slack: https://acme.slack.com/archives/C01/p17?ts=1_2 (see it).");

    assert_eq!(
        linked(&markdown),
        [
            unlinked("Slack: "),
            link("https://acme.slack.com/archives/C01/p17?ts=1_2"),
            unlinked(" (see it)."),
        ]
    );
}

#[test]
fn an_angle_bracket_url_is_a_link() {
    let markdown = Markdown::parse("Docs <https://x.test/d>.");

    assert_eq!(linked(&markdown)[1], link("https://x.test/d"));
}

#[test]
fn a_url_in_code_or_inside_a_link_stays_as_it_is() {
    let in_code = Markdown::parse("run `curl https://x.test/a`");
    let in_link = Markdown::parse("[see https://x.test/a](https://x.test/b)");

    assert_eq!(linked(&in_code)[1], unlinked("curl https://x.test/a"));
    assert_eq!(
        linked(&in_link),
        [(
            "see https://x.test/a".to_owned(),
            Some("https://x.test/b".to_owned())
        )]
    );
}

#[test]
fn a_url_keeps_a_closing_parenthesis_it_opened() {
    let markdown = Markdown::parse("see https://x.test/Rust_(language)");

    assert_eq!(linked(&markdown)[1], link("https://x.test/Rust_(language)"));
}

#[test]
fn headings_nested_one_level_down_shrink_and_stop_at_the_smallest() {
    let markdown = Markdown::parse("# Top\n\n###### Least\n\n> ## Quoted").nested_one_level();

    let levels: Vec<u8> = markdown
        .blocks
        .iter()
        .map(|block| match block {
            Block::Heading { level, .. } => *level,
            Block::Quote(inner) => match &inner[0] {
                Block::Heading { level, .. } => *level,
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(levels, [2, 6, 3]);
}

fn all_text(blocks: &[Block]) -> String {
    let mut text = String::new();
    for block in blocks {
        match block {
            Block::Paragraph(runs) | Block::Heading { text: runs, .. } => {
                text.extend(runs.iter().map(|run| run.text.as_str()));
                text.push('\n');
            }
            Block::List { items, .. } => items.iter().for_each(|item| text += &all_text(item)),
            Block::Quote(blocks) => text += &all_text(blocks),
            Block::Details { summary, body } => {
                text.extend(summary.iter().map(|run| run.text.as_str()));
                text += &all_text(body);
            }
            Block::Code { text: code, .. } => text += code,
            Block::Rule => {}
        }
    }
    text
}

#[test]
fn a_details_closed_inside_a_list_item_loses_no_text_after_it() {
    let markdown = Markdown::parse("<details>\n\n- item </details>\n- next\n\nafter\n\n# end");

    let text = all_text(&markdown.blocks);
    for expected in ["item", "next", "after", "end"] {
        assert!(
            text.contains(expected),
            "{expected} is missing from {text:?}"
        );
    }
}

#[test]
fn a_details_left_open_inside_a_list_item_ends_with_the_item() {
    let markdown = Markdown::parse("- one <details> two\n- three\n\nafter");

    let text = all_text(&markdown.blocks);
    for expected in ["one", "two", "three", "after"] {
        assert!(
            text.contains(expected),
            "{expected} is missing from {text:?}"
        );
    }
    assert!(matches!(markdown.blocks.last(), Some(Block::Paragraph(_))));
}
