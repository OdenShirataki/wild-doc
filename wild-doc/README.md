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
/*wd.run(br#"<wd:session name="hoge">
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
</wd:session>"#,b"").unwrap();*/

let update_xml=br#"<wd:session name="hoge">
<wd:update commit="1">
    <collection name="person">
        <field name="name"><wd:print value:var="input.name" /></field>
        <field name="country"><wd:print value:var="input.from" /></field>
    </collection>
</wd:update>
</wd:session>"#;
wd.run(update_xml,br#"{
    "name":"Noah"
    ,"from":"US"
}"#).unwrap();
wd.run(update_xml,br#"{
    "name":"Liam"
    ,"from":"US"
}"#).unwrap();
wd.run(update_xml,br#"{
    "name":"Olivia"
    ,"from":"UK"
}"#).unwrap();

//select data.
let r=wd.run(br#"
    <wd:search name="p" collection="person">
    </wd:search>
    <wd:result var="q" search="p" sort="field.name ASC,serial">
        <div>
            find <wd:print value:var="q.len" /> persons.
        </div>
        <ul>
            <wd:for var="person" in:var="q.rows"><li>
                <wd:print value:var="person.row" /> : <wd:print value:var="person.field.name" /> : <wd:print value:var="person.field.country" />
            </li></wd:for>
        </ul>
    </wd:result>
    <input type="text" name="hoge" />
    <wd:include src="body.xml" />
"#,b"").unwrap();
println!("{}",std::str::from_utf8(r.body()).unwrap());

//seaech data
let r=wd.run(br#"
    <wd:search name="p" collection="person">
        <field name="country" method="match" value="US" />
    </wd:search>
    <wd:result var="q" search="p">
        <div>
            find <wd:print value:var="q.len" /> persons from the US.
        </div>
        <ul>
            <wd:for var="person" in:var="q.rows"><li>
                <wd:print value:var="person.row" /> : <wd:print value:var="person.field.name" /> : <wd:print value:var="person.field.country" />
            </li></wd:for>
        </ul>
    </wd:result>
"#,b"").unwrap();
println!("{}",std::str::from_utf8(r.body()).unwrap());

//use javascript
let r=wd.run(br#"
    <?js
        const ymd=function(){
            const now=new Date();
            return now.getFullYear()+"-"+(now.getMonth()+1)+"-"+now.getDate();
        };
        wd.general.uk="UK";
        wd.general.ymd=function(){
            const now=new Date();
            return now.getFullYear()+"-"+(now.getMonth()+1)+"-"+now.getDate();
        };
        wd.result_options['test']="OK";
    ?>
    <wd:search name="p" collection="person">
        <field name="country" method="match" value:js="wd.general.uk" />
    </wd:search>
    <wd:result var="q" search="p">
        <div>
            <wd:print value:js="wd.general.ymd()" />
        </div>
        <div>
            find <wd:print value:js="q.len" /> persons from the <wd:print value:js="wd.general.uk" />.
        </div>
        <ul>
            <wd:for var="person" in:var="q.rows"><li>
                <wd:print value:var="person.row" /> : <wd:print value:var="person.field.name" /> : <wd:print value:var="personn.field.country" />
            </li></wd:for>
        </ul>
    </wd:result>
</wd>"#,b"").unwrap();
println!("{} : {}",std::str::from_utf8(r.body()).unwrap(),r.options_json());

//search in update section.
wd.run(br#"<wd:session name="hoge">
    <wd:update commit="1">
        <wd:search name="person" collection="person"></wd:search>
        <wd:result var="q" search="person">
            <wd:for var="r" in:var="q.rows">
                hoge:<wd:print value:var="r.row" />
                <collection name="person" row:var="r.row">
                    <field name="name">Renamed <wd:print value:var="r.field.name" /></field>
                    <field name="country"><wd:print value:var="r.field.country" /></field>
                </collection>
            </wd:for>
        </wd:result>
    </wd:update>
</wd:session>"#,b"").unwrap();
let r=wd.run(br#"
    <wd:search name="p" collection="person"></wd:search>
    <wd:result var="q" search="p">
        <div>
            find <wd:print value:var="q.len" /> persons.
        </div>
        <ul>
            <wd:for var="r" in:var="q.rows"><li>
                <wd:print value:var="r.row" /> : <wd:print value:var="r.field.name" /> : <wd:print value:var="r.field.country" />
            </li></wd:for>
        </ul>
    </wd:result>
"#,b"").unwrap();
println!("{}",std::str::from_utf8(r.body()).unwrap());

//use WebAPI
let r=wd.run(br#"
    <?js
        import { v4 as uuidv4 } from 'https://jspm.dev/uuid';
        console.log(uuidv4());

        wd.general.a="OK";
        wd.stack.push({
            hoge:{
                hoge:"A"
            }
            ,a:1
        });
        console.log(crypto.randomUUID());
        wd.result_options.test="TEST";
        wd.result_options.test2=crypto.randomUUID();
    ?>
    a:<wd:print value:js="wd.general.a" />
    v:<wd:print value:var="a" />
    input:<wd:print value:var="input.name" />
    <?js
        wd.stack.pop();
        wd.general.a="OK2";
        wd.general.b=1>2;
    ?>
    a:<wd:print value:js="wd.general.a" />
    v:<wd:print value:js="wd.general.b" />
"#,br#"{
    "name":"Ken"
    ,"from":"US"
}"#).unwrap();
println!("{} : {}",std::str::from_utf8(r.body()).unwrap(),r.options_json());
```

## Include file
### layout.xml
```xml
<html>
    <head>
        <title>HTML include test</title>
    </head>
    <body>
        <wd:include src:var="body_path" />
    </body>
</html>
```
### body.xml
```xml
BODY
```

### rust
```rust
let r=wd.run(br#"<wd:def body_path="body.xml">
    <wd:include src="layout.xml" />
<wd:def>"#,b"");
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

## Use python

Specify features in Cargo.toml.
```toml
wild-doc = { version = "x" , path = "../wild-doc" ,features=[ "js","py" ] }
```

### code
```rust
//use WebAPI
let r=wd.run(br#"<?py
hoge=100
def get_200():
    return 200
?><wd:print value:py="get_200()" />"#,b"").unwrap();
```