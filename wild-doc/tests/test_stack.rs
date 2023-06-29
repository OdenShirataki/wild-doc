#[cfg(test)]
#[test]
fn test_stack() {
    use wild_doc::*;

    let dir = "./wd-test/";
    if std::path::Path::new(dir).exists() {
        std::fs::remove_dir_all(dir).unwrap();
    }
    std::fs::create_dir_all(dir).unwrap();

    let mut wd = WildDoc::new(dir, Box::new(IncludeLocal::new("./include/"))).unwrap();

    let xml = br#"<?js
        wd.general.script_var=[1,2,3,4];
    ?><wd:local
        hoge="1"
        hoge2="hoge"
        hoge3="true"
        hoge4="2.3"
        hoge5="[1,2]"
        hoge6="{&quot;hoge&quot;:1,&quot;hoge2&quot;:3}"
    >
        <wd:for var="i" in:var="hoge6">
            for:<wd:print value:var="i" />
        </for>
        <wd:for var="i" in:js="wd.general.script_var">
            script_for:<wd:print value:var="i" />
        </for>
        <wd:print value:var="hoge" />
        <wd:print value:var="hoge2" />
        <wd:print value:var="hoge3" />
        <wd:print value:var="hoge4" />
        <wd:print value:var="hoge5" />
        <wd:print value:var="hoge6" />
    </wd:local>"#;
    let r = wd.run(xml, b"").unwrap();
    println!("{}", std::str::from_utf8(r.body()).unwrap());
}
