//! Condition tags inside wd:search.

#[cfg(feature = "js")]
#[test]
fn wide_and_join_tags() {
    use wild_doc::*;

    let dir = std::env::temp_dir().join("wild-doc-search-tags");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut wd = WildDoc::new(&dir, IncludeLocal::new("./include/"), None, 1);
    wd.run(
        br#"<wd:session name="s"><wd:update commit="true">
            <collection name="person"><field name="name">Noah</field></collection>
            <collection name="person"><field name="name">Liam</field></collection>
        </wd:update></wd:session>"#,
        b"",
    )
    .unwrap();

    let mut count = |conditions: &str| {
        let xml = format!(
            r#"<wd:search collection="person">{conditions}<result var="p"><wd:print value:js="wd.v('p').rows.length" /></result></wd:search>|after"#
        );
        String::from_utf8(wd.run(xml.as_bytes(), b"").unwrap().body().to_vec()).unwrap()
    };

    // wide used to read its conditions from the wrong position.
    assert_eq!(
        count(
            r#"<wide><field name="name" method="match" value="Noah" /><field name="name" method="match" value="Liam" /></wide>"#
        ),
        "2|after"
    );
    assert_eq!(
        count(r#"<wide><field name="name" method="match" value="X" /></wide>"#),
        "0|after"
    );
    // a join to a collection that does not exist used to end the search early.
    assert_eq!(
        count(r#"<join name="h" collection="nothing" relation="person"></join>"#),
        "2|after"
    );
}
