#[cfg(test)]
#[cfg(feature = "py")]
#[test]
fn test_py() {
    use wild_doc::*;

    let dir = "./wd-test/";
    if std::path::Path::new(dir).exists() {
        std::fs::remove_dir_all(dir).unwrap();
    }
    std::fs::create_dir_all(dir).unwrap();

    let mut wd = WildDoc::new(dir, IncludeLocal::new("./include/"), None, 1);

    let xml = br#"<?py
hoge=100
def get_200():
    return 200
?><wd:print value:py="get_200()" />"#;
    let r = wd.run(xml, b"").unwrap();
    println!("{}", std::str::from_utf8(r.body()).unwrap());
    assert_eq!(r.body(), b"200");

    // v() reads variables of wild-doc from Python.
    let xml = br#"<wd:print value:py="v('input')['name'] + '!'" />"#;
    let r = wd.run(xml, br#"{"name":"Noah"}"#).unwrap();
    assert_eq!(r.body(), b"Noah!");

    // an unknown variable is an empty dict.
    let xml = br#"<wd:print value:py="len(v('nothing'))" />"#;
    let r = wd.run(xml, b"").unwrap();
    assert_eq!(r.body(), b"0");
}
