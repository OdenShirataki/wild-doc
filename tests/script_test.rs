#[cfg(test)]
#[test]
fn script_test() {
    use wild_doc::*;

    let dir = "./script-test/";
    if !std::path::Path::new(dir).exists() {
        std::fs::create_dir_all(dir).unwrap();
    }

    let mut wd = WildDoc::new(dir, IncludeLocal::new("./include/")).unwrap();

    let r = wd
        .run(
            r#"<wd>
    <wd:script>
        import { Image, decode } from "https://deno.land/x/imagescript@1.2.15/mod.ts";
        console.log("TEST");
        const data=await decode(await Deno.readFile('C:\\Users\\18kbg\\Pictures\\small.jpg'));
        console.log(data);
    </wd:script>
</wd>"#,
            r#""#,
        )
        .unwrap();
    println!(
        "{} : {}",
        std::str::from_utf8(r.body()).unwrap(),
        r.options_json()
    );

}
