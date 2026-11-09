//! Search results handed to JavaScript stay usable after the variable that
//! held them went out of scope.

#[cfg(feature = "js")]
#[test]
fn search_result_kept_by_javascript() {
    use wild_doc::*;

    let dir = std::env::temp_dir().join("wild-doc-deno-results");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut wd = WildDoc::new(&dir, IncludeLocal::new("./include/"), None, 1);

    wd.run(
        br#"<wd:session name="s"><wd:update commit="true">
            <collection name="person"><field name="name">Noah</field></collection>
        </wd:update></wd:session>"#,
        b"",
    )
    .unwrap();
    wd.run(
        br#"<wd:session name="s"><wd:update commit="true">
            <collection name="history">
                <field name="event">Birth</field>
                <depend key="person" collection="person" row="1" />
            </collection>
            <collection name="history">
                <field name="event">Marriage</field>
                <depend key="person" collection="person" row="1" />
            </collection>
        </wd:update></wd:session>"#,
        b"",
    )
    .unwrap();

    let r = wd
        .run(
            br#"<wd:search collection="person"><join name="h" collection="history" relation="person"></join><result var="p"><?js
                wd.general.p = wd.v("p");
            ?></result></wd:search>[<wd:print value:js="wd.general.p.rows.length" />,<wd:print value:js="wd.general.p.join('h', wd.general.p.rows[0]).rows.length" />,<wd:print value:js="wd.general.p.join('h', 999) === undefined" />]"#,
            b"",
        )
        .unwrap();
    assert_eq!(std::str::from_utf8(r.body()).unwrap(), "[1,2,true]");
}
