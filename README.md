# wild-doc

## Example

```rust
use wild_doc::*;

let dir="./wd-test/";
if std::path::Path::new(dir).exists(){
    std::fs::remove_dir_all(dir).unwrap();
}
std::fs::create_dir_all(dir).unwrap();

let mut wd=WildDoc::new(
    dir
    ,IncludeLocal::new("./include/")
).unwrap();

//update data.
/*wd.exec(r#"<wd><wd:session name="hoge">
    <wd:update commit="1">
        <collection name="person">
            <field name="name">Noah</field>
            <field name="country">US</field>
        </collection>
        <collection name="person">
            <field name="name">Liam</field>
            <field name="country">US</field>
        </collection>
        <collection name="person">
            <field name="name">Olivia</field>
            <field name="country">UK</field>
        </collection>
    </wd:update>
</wd:session></wd>"#,b"").unwrap();*/

let update_xml=r#"<wd><wd:session name="hoge">
    <wd:update commit="1">
        <collection name="person">
            <field name="name"><wd:print wd:value="wd.input.name" /></field>
            <field name="country"><wd:print wd:value="wd.input.from" /></field>
        </collection>
    </wd:update>
</wd:session></wd>"#;
wd.exec(update_xml,r#"{
    "name":"Noah"
    ,"from":"US"
}"#.as_bytes()).unwrap();
wd.exec(update_xml,r#"{
    "name":"Liam"
    ,"from":"US"
}"#.as_bytes()).unwrap();
wd.exec(update_xml,r#"{
    "name":"Olivia"
    ,"from":"UK"
}"#.as_bytes()).unwrap();

//select data.
let r=wd.exec(r#"<wd>
    <wd:search name="p" collection="person">
    </wd:search>
    <wd:result var="q" search="p">
        <div>
            find <wd:print wd:value="wd.v('q').length" /> persons.
        </div>
        <ul>
            <wd:for var="r" index="i" wd:in="wd.v('q')"><li>
                <wd:print wd:value="wd.v('r').row" /> : <wd:print wd:value="wd.v('r').field('name')" /> : <wd:print wd:value="wd.v('r').field('country')" />
            </li></wd:for>
        </ul>
    </wd:result>
    <input type="text" name="hoge" />
    <wd:include src="body.xml" />
</wd>"#,b"").unwrap();
println!("{}",std::str::from_utf8(r.body()).unwrap());

//seaech data
let r=wd.exec(r#"<wd>
    <wd:search name="p" collection="person">
        <field name="country" method="match" value="US" />
    </wd:search>
    <wd:result var="q" search="p">
        <div>
            find <wd:print wd:value="wd.v('q').length" /> persons from the US.
        </div>
        <ul>
            <wd:for var="r" index="i" wd:in="wd.v('q')"><li>
                <wd:print wd:value="wd.v('r').row" /> : <wd:print wd:value="wd.v('r').field('name')" /> : <wd:print wd:value="wd.v('r').field('country')" />
            </li></wd:for>
        </ul>
    </wd:result>
</wd>"#,b"").unwrap();
println!("{}",std::str::from_utf8(r.body()).unwrap());

//use javascript
let r=wd.exec(r#"<wd>
    <wd:script>
        const ymd=function(){
            const now=new Date();
            return now.getFullYear()+"-"+(now.getMonth()+1)+"-"+now.getDate();
        };
        const uk="UK";

        wd.result_options['test']="OK";
    </wd:script>
    <wd:search name="p" collection="person">
        <field name="country" method="match" wd:value="uk" />
    </wd:search>
    <wd:result var="q" search="p">
        <div>
            <wd:print wd:value="ymd()" />
        </div>
        <div>
            find <wd:print wd:value="wd.v('q').length" /> persons from the <wd:print wd:value="uk" />.
        </div>
        <ul>
            <wd:for var="r" index="i" wd:in="wd.v('q')"><li>
                <wd:print wd:value="wd.v('r').row" /> : <wd:print wd:value="wd.v('r').field('name')" /> : <wd:print wd:value="wd.v('r').field('country')" />
            </li></wd:for>
        </ul>
    </wd:result>
</wd>"#,b"").unwrap();
println!("{} : {}",std::str::from_utf8(r.body()).unwrap(),r.options_json());

//search in update section.
wd.exec(r#"<wd><wd:session name="hoge">
    <wd:update commit="1">
        <wd:search name="person" collection="person"></wd:search>
        <wd:result var="q" search="person">
            <wd:for var="r" index="i" wd:in="wd.v('q')">
                hoge:<wd:print wd:value="wd.v('r').row" />
                <collection name="person" wd:row="wd.v('r').row">
                    <field name="name">Renamed <wd:print wd:value="wd.v('r').field('name')" /></field>
                    <field name="country"><wd:print wd:value="wd.v('r').field('country')" /></field>
                </collection>
            </wd:for>
        </wd:result>
    </wd:update>
</wd:session></wd>"#,b"").unwrap();
let r=wd.exec(r#"<wd>
    <wd:search name="p" collection="person"></wd:search>
    <wd:result var="q" search="p">
        <div>
            find <wd:print wd:value="wd.v('q').length" /> persons.
        </div>
        <ul>
            <wd:for var="r" index="i" wd:in="wd.v('q')"><li>
                <wd:print wd:value="wd.v('r').row" /> : <wd:print wd:value="wd.v('r').field('name')" /> : <wd:print wd:value="wd.v('r').field('country')" />
            </li></wd:for>
        </ul>
    </wd:result>
</wd>"#,b"").unwrap();
println!("{}",std::str::from_utf8(r.body()).unwrap());
```

## Include file
### layout.xml
```xml
<html>
    <head>
        <title>HTML include test</title>
    </head>
    <body>
        <wd:include wd:src="wd.v('body_path')" />
    </body>
</html>
```
### body.xml
```xml
BODY
```

### rust
```rust
let r=wd.exec(r#"<wd><wd:stack var="body_path:'body.xml'">
    <wd:include src="layout.xml" />
<wd:stack></wd>"#);
    println!("{}",r);
```

### output
```html
<html>
    <head>
        <title>HTML include test</title>
    </head>
    <body>
        BODY
    </body>
</html>
```