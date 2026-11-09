//! Inputs from templates that used to cause undefined behavior or panics.

use wild_doc::*;

fn new_wd(name: &str) -> WildDoc<IncludeLocal> {
    let dir = std::env::temp_dir().join(format!("wild-doc-inputs-{name}"));
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
    wd
}

fn record(wd: &mut WildDoc<IncludeLocal>, row: &str) -> String {
    let xml = format!(
        r#"<wd:record var="r" collection="person" row="{row}">[<wd:print value:var="r.field.name" />]</wd:record>"#
    );
    String::from_utf8(wd.run(xml.as_bytes(), b"").unwrap().body().to_vec()).unwrap()
}

#[test]
fn record_with_rows_out_of_range() {
    let mut wd = new_wd("record");
    assert_eq!(record(&mut wd, "1"), "[Noah]");
    // 2^32 + 1 used to be truncated to row 1, and rows that do not exist
    // were read without checking.
    for row in ["4294967297", "4294967296", "9223372036854775807", "99999"] {
        assert_ne!(record(&mut wd, row), "[Noah]", "row {row}");
    }
}

#[test]
fn invalid_utf8_template_is_an_error() {
    let mut wd = new_wd("utf8");
    assert!(wd.run(b"<div>\xff\xfe</div>", b"").is_err());
}

#[test]
fn update_with_rows_out_of_range_is_an_error() {
    let mut wd = new_wd("update");
    for row in ["4294967297", "-9223372036854775808"] {
        let xml = format!(
            r#"<wd:session name="s"><wd:update commit="true">
                <collection name="person" row="{row}"><field name="name">X</field></collection>
            </wd:update></wd:session>"#
        );
        assert!(wd.run(xml.as_bytes(), b"").is_err(), "row {row}");
    }
    // the existing row was not touched.
    assert_eq!(record(&mut wd, "1"), "[Noah]");
}
