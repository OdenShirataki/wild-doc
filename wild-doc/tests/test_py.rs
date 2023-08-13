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

    let mut wd = WildDoc::new(dir, Box::new(IncludeLocal::new("./include/")), None);

    let xml = br#"<?py
hoge=100
def get_200():
    return 200
?><wd:print value:py="get_200()" />"#;
    let r = wd.run(xml, b"").unwrap();
    println!("{}", std::str::from_utf8(r.body()).unwrap());
}
