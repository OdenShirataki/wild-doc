#[cfg(test)]
#[test]
fn test1() {
    use wild_doc::*;

    let dir = "./wd-test/";
    if std::path::Path::new(dir).exists() {
        std::fs::remove_dir_all(dir).unwrap();
    }
    std::fs::create_dir_all(dir).unwrap();

    let mut wd = WildDoc::new(dir, IncludeLocal::new("./include/"), None, 1);

    let update_xml = br#"<wd:session name="account"><wd:update commit="true">
    <collection name="account">
        <field name="id">admin</field>
        <field name="password">admin</field>
    </collection>
</wd:update></wd:session>"#;
    wd.run(update_xml, b"").unwrap();

    let r=wd.run(br#"<?js
    wd.general.test={a:1,b:2,c:3};
    console.log(wd.general.test);
?><wd:for
    var="aa" key="key" in:js="(()=>{return {a:1,b:2,c:3};})()"
><wd:print value:var="key" /> : <wd:print value:var="aa" />
</wd:for><wd:session name="logintest">
    <wd:update commit="false">
        <collection name="login">
            <field name="test">hoge</field>
            <depend key="account" collection="account" row="1" />
        </collection>
    </wd:update>
    <wd:search
        collection="login"
    ><result
        var="login"
    ><wd:for var="row" in:var="login.rows"><wd:record var="row" collection="login" row:var="row">
        <wd:print value:var="row.row" /> : <wd:print value:var="row.uuid" /> : <wd:print value:var="row.field.test" /> : <wd:print value:var="row.depends.account" />
        <wd:search
            collection="account"
        ><row in:var="row.depends.account.row"><result
            var="account"
        ><wd:for var="a" in:var="account.rows"><wd:record var="a" collection="account" row:var="a">
            dep:<wd:print value:var="a.field.id" />@<wd:print value:var="a.field.password" />
        </wd:record></wd:for></result></wd:search>
    </wd:record></wd:for></result></wd:search>
</wd:session>"#
        ,b""
    ).unwrap();
    println!("{}", std::str::from_utf8(r.body()).unwrap());

    let update_xml = br#"<wd:session name="logintest" clear_on_close="true"></wd:session>"#;
    wd.run(update_xml, b"").unwrap();

    //update data.
    /*wd.run(r#"<wd:session name="hoge">
        <wd:update commit="true">
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

    let update_xml = br#"<wd:session name="hoge">
    <wd:update commit="true">
        <collection name="person">
            <field name="name"><wd:print value:var="input.name" /></field>
            <field name="country"><wd:print value:var="input.from" /></field>
        </collection>
    </wd:update>
</wd:session>"#;
    wd.run(
        update_xml,
        br#"{
    "name":"Noah"
    ,"from":"US"
}"#,
    )
    .unwrap();
    wd.run(
        update_xml,
        br#"{
    "name":"Liam"
    ,"from":"US"
}"#,
    )
    .unwrap();
    wd.run(
        update_xml,
        br#"{
    "name":"Olivia"
    ,"from":"UK"
}"#,
    )
    .unwrap();

    //select data.
    let r=wd.run(br#"
    <wd:search collection="person"><result var="p">
        <div>
            find <wd:print value:var="p.len" /> persons.
        </div>
        <ul>
            <wd:for var="person" in:var="p.rows"><wd:record var="person" collection="person" row:var="person"><li>
                <wd:print value:var="person.row" /> : <wd:print value:var="person.activity" /> : <wd:print value:var="person.uuid" /> : <wd:print value:var="person.field.name" /> : <wd:print value:var="person.field.country" />
            </li></wd:record></wd:for>
        </ul>
    </result></wd:search>
    <input type="text" name="hoge" />
    <wd:include src="body.xml" />
"#,b"").unwrap();
    println!("{}", std::str::from_utf8(r.body()).unwrap());

    //seaech data
    let r=wd.run(br#"
        <wd:search collection="person"><field name="country" method="match" value="US" /><result var="p">
            <div>
                find <wd:print value:var="p.len" /> persons from the US.
            </div>
            <ul>
                <wd:for
                    var="person"
                    in:var="p.rows"
                ><wd:record var="person" collection="person" row:var="person"><li>
                    <wd:print value:var="person.row" /> : <wd:print value:var="person.field.name" /> : <wd:print value:var="person.field.country" />
                </li></wd:record></wd:for>
            </ul>
        </result></wd:search>
    "#,b"").unwrap();
    println!("{}", std::str::from_utf8(r.body()).unwrap());

    //use javascript
    let r=wd.run(br#"
        <?js
            wd.general.result_options={};

            const ymd=function(){
                const now=new Date();
                return now.getFullYear()+"-"+(now.getMonth()+1)+"-"+now.getDate();
            };
            wd.general.uk="UK";
            wd.general.ymd=function(){
                const now=new Date();
                return now.getFullYear()+"-"+(now.getMonth()+1)+"-"+now.getDate();
            };
            wd.general.result_options['test']="OK";
            let hoge=wd.get_contents('body.xml');
            console.log("hoge",hoge);
        ?>
        <wd:search collection="person"><field name="country" method="match" value:js="wd.general.uk" /><result var="p">
            <div>
                <wd:print value:js="wd.general.ymd()" />
            </div>
            <div>
                find <wd:print value:var="p.len" /> persons from the <wd:print value:js="wd.general.uk" />.
            </div>
            <ul>
                <wd:for var="person" in:var="p.rows"><wd:record var="person" collection="person" row:var="person"><li>
                    <wd:print value:var="person.row" /> : <wd:print value:var="person.field.name" /> : <wd:print value:var="person.field.country" />
                </li></wd:record></wd:for>
            </ul>
        </result></wd:search>
    "#,b"").unwrap();
    println!(
        "{} : {:#?}",
        std::str::from_utf8(r.body()).unwrap(),
        r.options()
    );

    //search in update section.
    let r=wd.run(br#"<wd:session name="hoge">
        <wd:update commit="true">
            <wd:search
                collection="person"
            ><result var="p">
                <wd:for
                    var="person" in:var="p.rows"
                ><wd:record var="person" collection="person" row:var="person">
                    <collection name="person" row:var="person.row">
                        <field name="name">Renamed <wd:print value:var="person.field.name" /></field>
                        <field name="country"><wd:print value:var="person.field.country" /></field>
                    </collection>
                </wd:record></wd:for>
            </result></wd:search>
        </wd:update>
    </wd:session>"#,b"").unwrap();
    println!("{}", std::str::from_utf8(r.body()).unwrap());
    let r=wd.run(br#"
        <wd:search collection="person"><result var="p">
            <div>
                find <wd:print value:var="p.len" /> persons.
            </div>
            <ul>
                <wd:for
                    var="person" in:var="p.rows"
                ><wd:record var="person" collection="person" row:var="person"><li>
                    <wd:print value:var="person.row" /> : <wd:print value:var="person.field.name" /> : <wd:print value:var="person.field.country" />
                </li></wd:record></wd:for>
            </ul>
        </result></wd:search>
    "#,b"").unwrap();
    println!("{}", std::str::from_utf8(r.body()).unwrap());

    //use WebAPI
    let r = wd
        .run(
            br#"
        <?js
            import { v4 as uuidv4 } from 'https://jspm.dev/uuid';
            console.log(uuidv4());

            wd.general.result_options={};
        
            wd.general.a="OK";
            console.log(crypto.randomUUID());
            wd.general.result_options.test="TEST";
            wd.general.result_options.test2=crypto.randomUUID();
        ?>
        a:<wd:print value:js="wd.general.a" />
        input:<wd:print value:js="wd.v('input').name" />
        <?js
            wd.general.a="OK2";
            wd.general.b=1>2;
        ?>
        a:<wd:print value:js="wd.general.a" />
        v:<wd:print value:js="wd.general.b" />
    "#,
            br#"{
        "name":"Ken"
        ,"from":"US"
    }"#,
        )
        .unwrap();
    println!(
        "{} : {:#?}",
        std::str::from_utf8(r.body()).unwrap(),
        r.options()
    );
}
