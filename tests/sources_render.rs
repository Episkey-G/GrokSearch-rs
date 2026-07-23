use grok_search_rs::sources::arxiv::{render as arxiv_render, ArxivExtractor};
use grok_search_rs::sources::github::{
    parse_release as gh_parse_release, render as gh_render, render_release as gh_render_release,
    GithubIssueExtractor, GithubPrExtractor, GithubRaw, GithubReleaseExtractor, GithubReleaseRaw,
};
use grok_search_rs::sources::stackexchange::{render as se_render, SeRaw, StackExchangeExtractor};
use grok_search_rs::sources::wikipedia::{
    parse_page as wiki_parse_page, render as wiki_render, WikiRaw, WikipediaExtractor,
};
use grok_search_rs::sources::{SourceCaps, SourceExtractor};
use url::Url;

fn issue_fixture() -> GithubRaw {
    serde_json::from_str(include_str!("fixtures/sources/github_issue.json")).unwrap()
}

fn pr_fixture() -> GithubRaw {
    serde_json::from_str(include_str!("fixtures/sources/github_pr.json")).unwrap()
}

#[test]
fn github_issue_render_shows_title_and_state() {
    let out = gh_render(&issue_fixture(), &SourceCaps::default());
    assert!(out.contains("Fix segfault in parser"), "title: {out}");
    assert!(out.contains("open"), "state: {out}");
}

#[test]
fn github_issue_render_shows_labels() {
    let out = gh_render(&issue_fixture(), &SourceCaps::default());
    assert!(out.contains("bug"));
    assert!(out.contains("good first issue"));
}

#[test]
fn github_issue_render_folds_comments_at_cap() {
    let caps = SourceCaps {
        max_answers: 5,
        max_comments: 2,
    };
    let out = gh_render(&issue_fixture(), &caps);
    assert!(out.contains("还有 1 条评论"), "fold: {out}");
}

#[test]
fn github_pr_render_shows_merged_state() {
    let out = gh_render(&pr_fixture(), &SourceCaps::default());
    assert!(out.contains("merged"), "pr: {out}");
}

#[test]
fn github_matcher_strict_positive_and_negative() {
    let issue = GithubIssueExtractor { token: None };
    let pr = GithubPrExtractor { token: None };
    let m = |u: &str| Url::parse(u).unwrap();

    assert!(issue.matches(&m("https://github.com/owner/repo/issues/42")));
    assert!(pr.matches(&m("https://github.com/owner/repo/pull/7")));

    for neg in [
        "https://github.com/",
        "https://github.com/owner/repo/issues",
        "https://github.com/owner/repo/pull/7/files",
        "https://gist.github.com/user/abc",
        "https://github.com/owner/repo/discussions/1",
        "https://github.com/owner/repo/blob/main/README.md",
        "https://github.com/owner/repo/releases/tag/v1.0.0",
    ] {
        assert!(!issue.matches(&m(neg)), "issue should reject {neg}");
        assert!(!pr.matches(&m(neg)), "pr should reject {neg}");
    }
}

fn release_fixture() -> GithubReleaseRaw {
    let json: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/sources/github_release.json")).unwrap();
    gh_parse_release(&json).expect("parse")
}

#[test]
fn github_release_parse_extracts_tag_notes_and_metadata() {
    let raw = release_fixture();
    assert_eq!(raw.tag, "rmcp-v2.2.0");
    assert_eq!(raw.name, "rmcp v2.2.0");
    assert_eq!(raw.author, "alice");
    assert_eq!(raw.published_at, "2026-07-18T09:30:00Z");
    assert!(!raw.prerelease);
    assert!(raw.body.contains("streamable HTTP transport"));
}

#[test]
fn github_release_parse_rejects_payload_without_tag() {
    let json = serde_json::json!({ "name": "x", "body": "notes" });
    assert!(gh_parse_release(&json).is_err());
}

#[test]
fn github_release_render_shows_title_tag_date_and_notes() {
    let out = gh_render_release(&release_fixture(), &SourceCaps::default());
    assert!(out.contains("# rmcp v2.2.0"), "title: {out}");
    assert!(out.contains("**Tag:** rmcp-v2.2.0"), "tag: {out}");
    assert!(
        out.contains("**Published:** 2026-07-18T09:30:00Z"),
        "date: {out}"
    );
    assert!(out.contains("**Author:** alice"), "author: {out}");
    assert!(out.contains("What's Changed"), "notes: {out}");
}

#[test]
fn github_release_render_falls_back_to_tag_title_and_marks_prerelease() {
    let raw = GithubReleaseRaw {
        tag: "v0.9.0-rc.1".into(),
        name: "  ".into(),
        author: "alice".into(),
        published_at: String::new(),
        prerelease: true,
        body: "release candidate".into(),
    };
    let out = gh_render_release(&raw, &SourceCaps::default());
    assert!(out.contains("# v0.9.0-rc.1"), "tag title: {out}");
    assert!(out.contains("(prerelease)"), "prerelease marker: {out}");
    assert!(
        !out.contains("**Published:**"),
        "empty date must fold: {out}"
    );
}

#[test]
fn github_release_matcher_tag_and_latest_positive() {
    let rel = GithubReleaseExtractor { token: None };
    let m = |u: &str| Url::parse(u).unwrap();
    assert!(rel.matches(&m("https://github.com/owner/repo/releases/tag/v1.2.3")));
    assert!(rel.matches(&m(
        "https://github.com/modelcontextprotocol/rust-sdk/releases/tag/rmcp-v2.2.0"
    )));
    assert!(rel.matches(&m("https://github.com/owner/repo/releases/latest")));
}

#[test]
fn github_release_matcher_negative() {
    let rel = GithubReleaseExtractor { token: None };
    let m = |u: &str| Url::parse(u).unwrap();
    for neg in [
        "https://github.com/owner/repo/releases",
        "https://github.com/owner/repo/releases/tag/",
        "https://github.com/owner/repo/releases/download/v1.0/pkg.tar.gz",
        "https://github.com/owner/repo/releases/tag/v1.0/extra",
        "https://github.com/owner/repo/releases/latest/extra",
        "https://github.com/owner/repo/issues/42",
        "https://gist.github.com/user/abc",
    ] {
        assert!(!rel.matches(&m(neg)), "should reject {neg}");
    }
}

fn se_fixture() -> SeRaw {
    serde_json::from_str(include_str!("fixtures/sources/stackexchange.json")).unwrap()
}

#[test]
fn se_render_accepted_answer_marked_and_first() {
    let out = se_render(&se_fixture(), &SourceCaps::default());
    let star = out.find("★ 采纳答案").expect("accepted marker present");
    let dave = out.find("dave").expect("non-accepted author present");
    assert!(star < dave, "accepted answer must come before non-accepted");
}

#[test]
fn se_render_folds_extra_answers_at_cap() {
    let caps = SourceCaps {
        max_answers: 2,
        max_comments: 30,
    };
    let out = se_render(&se_fixture(), &caps);
    assert!(out.contains("还有 2 条"), "fold: {out}");
}

#[test]
fn se_render_accepted_answer_includes_comments() {
    let out = se_render(&se_fixture(), &SourceCaps::default());
    assert!(out.contains("Also `list[::-1]`"), "accepted comment: {out}");
}

#[test]
fn se_render_other_answers_have_no_comments() {
    let out = se_render(&se_fixture(), &SourceCaps::default());
    let after_first_non_accepted = out.split("## 答案").nth(1).unwrap_or("");
    assert!(
        !after_first_non_accepted.contains("> **"),
        "non-accepted answers must render no comments"
    );
}

#[test]
fn se_matcher_full_network_positive() {
    let se = StackExchangeExtractor;
    let m = |u: &str| Url::parse(u).unwrap();
    for pos in [
        "https://stackoverflow.com/questions/1234/how-do-i",
        "https://serverfault.com/questions/99",
        "https://superuser.com/questions/5678",
        "https://askubuntu.com/questions/111",
        "https://mathoverflow.net/questions/222",
        "https://math.stackexchange.com/questions/333",
        "https://codereview.stackexchange.com/questions/444",
    ] {
        assert!(se.matches(&m(pos)), "should match {pos}");
    }
}

#[test]
fn se_matcher_non_question_negative() {
    let se = StackExchangeExtractor;
    let m = |u: &str| Url::parse(u).unwrap();
    for neg in [
        "https://stackoverflow.com/users/42/alice",
        "https://stackoverflow.com/tags/rust",
        "https://stackoverflow.com/questions",
    ] {
        assert!(!se.matches(&m(neg)), "should reject {neg}");
    }
}

const ARXIV_FIXTURE: &str = include_str!("fixtures/sources/arxiv_atom.xml");

#[test]
fn arxiv_parse_atom_returns_title() {
    let raw = ArxivExtractor::parse_atom(ARXIV_FIXTURE).expect("parse");
    assert_eq!(raw.title, "Attention Is All You Need");
}

#[test]
fn arxiv_parse_atom_lists_all_authors() {
    let raw = ArxivExtractor::parse_atom(ARXIV_FIXTURE).expect("parse");
    assert!(raw.authors.iter().any(|a| a == "Ashish Vaswani"));
    assert!(raw.authors.len() >= 2);
}

#[test]
fn arxiv_parse_atom_returns_categories() {
    let raw = ArxivExtractor::parse_atom(ARXIV_FIXTURE).expect("parse");
    assert!(raw.categories.iter().any(|c| c == "cs.CL"));
}

#[test]
fn arxiv_parse_atom_returns_pdf_link() {
    let raw = ArxivExtractor::parse_atom(ARXIV_FIXTURE).expect("parse");
    assert!(raw.pdf_link.contains("pdf"), "pdf_link: {}", raw.pdf_link);
}

#[test]
fn arxiv_render_shows_title_and_pdf_link() {
    let raw = ArxivExtractor::parse_atom(ARXIV_FIXTURE).expect("parse");
    let out = arxiv_render(&raw, &SourceCaps::default());
    assert!(out.contains("# Attention Is All You Need"));
    assert!(out.contains("[PDF]"));
    assert!(out.contains("[Abstract]"));
}

#[test]
fn arxiv_matcher_positive_abs_and_pdf() {
    let ax = ArxivExtractor;
    let m = |u: &str| Url::parse(u).unwrap();
    assert!(ax.matches(&m("https://arxiv.org/abs/1706.03762")));
    assert!(ax.matches(&m("https://arxiv.org/pdf/1706.03762")));
    assert!(ax.matches(&m("https://arxiv.org/abs/2106.09685v2")));
}

#[test]
fn arxiv_matcher_negative_non_paper_paths() {
    let ax = ArxivExtractor;
    let m = |u: &str| Url::parse(u).unwrap();
    assert!(!ax.matches(&m("https://arxiv.org/")));
    assert!(!ax.matches(&m("https://arxiv.org/search/")));
    assert!(!ax.matches(&m("https://export.arxiv.org/api/query?id_list=1706.03762")));
}

#[test]
fn wiki_render_shows_title_and_body() {
    let raw = WikiRaw {
        title: "Rust (programming language)".into(),
        extract: "Rust achieves memory safety without a garbage collector.".into(),
        lang: "en".into(),
    };
    let out = wiki_render(&raw, &SourceCaps::default());
    assert!(out.contains("# Rust (programming language)"));
    assert!(out.contains("memory safety"));
}

#[test]
fn wiki_render_produces_clean_plaintext() {
    let raw = WikiRaw {
        title: "Rust".into(),
        extract: "Plain text, no markup.".into(),
        lang: "en".into(),
    };
    let out = wiki_render(&raw, &SourceCaps::default());
    assert!(
        !out.contains('<'),
        "render must not contain HTML tags: {out}"
    );
}

#[test]
fn wiki_matcher_positive_article_urls() {
    let w = WikipediaExtractor;
    let m = |u: &str| Url::parse(u).unwrap();
    assert!(w.matches(&m(
        "https://en.wikipedia.org/wiki/Rust_(programming_language)"
    )));
    assert!(w.matches(&m("https://fr.wikipedia.org/wiki/Rust_(langage)")));
}

#[test]
fn wiki_matcher_excludes_all_non_article_namespaces() {
    let w = WikipediaExtractor;
    let m = |u: &str| Url::parse(u).unwrap();
    for neg in [
        "https://en.wikipedia.org/wiki/Special:Search",
        "https://en.wikipedia.org/wiki/Talk:Rust",
        "https://en.wikipedia.org/wiki/Category:Programming_languages",
        "https://en.wikipedia.org/wiki/Help:Contents",
        "https://en.wikipedia.org/wiki/User:Alice",
        "https://en.wikipedia.org/wiki/File:Rust_logo.png",
        "https://en.wikipedia.org/wiki/Template:Infobox",
        "https://en.wikipedia.org/wiki/Wikipedia:About",
        "https://en.wikipedia.org/wiki/Draft:Article",
        "https://en.wikipedia.org/wiki/Portal:Science",
    ] {
        assert!(!w.matches(&m(neg)), "should exclude {neg}");
    }
}

#[test]
fn wiki_parse_page_extracts_title_body_and_lang() {
    let json: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/sources/wikipedia_extract.json")).unwrap();
    let raw = wiki_parse_page(&json, "en").expect("parse");
    assert_eq!(raw.title, "Rust (programming language)");
    assert!(raw.extract.contains("memory safety"));
    assert_eq!(raw.lang, "en");
}
