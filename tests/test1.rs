#[cfg(test)]
#[test]
fn test1() {
    use wild_doc::*;

    let dir = "./wd-test/";
    if std::path::Path::new(dir).exists() {
        std::fs::remove_dir_all(dir).unwrap();
    }
    std::fs::create_dir_all(dir).unwrap();

    let mut wd = WildDoc::new(dir, IncludeLocal::new("./include/")).unwrap();

    let update_xml = r#"<wd><wd:session name="account"><wd:update commit="1">
        <collection name="account">
            <field name="id">admin</field>
            <field name="password">admin</field>
        </collection>
    </wd:update></wd:session></wd>"#;
    wd.run(update_xml, "").unwrap();

    let r=wd.run(r#"<wd><wd:session name="logintest">
        <wd:update commit="0">
            <collection name="login">
                <field name="test">hoge</field>
                <depend key="account" collection="account" row="1" />
            </collection>
        </wd:update>
        <wd:search name="login" collection="login">
        </wd:search><wd:result var="login" search="login"><wd:for var="r" index="i" wd:in="wd.v('login')">
            <wd:print wd:value="wd.v('r').row" /> : <wd:print wd:value="wd.v('r').field('test')" /><wd:script>
                console.log(wd.v('r').depends('account'));
            </wd:script>
            <wd:for var="dep" index="i" wd:in="wd.v('r').depends('account')">
                dep:<wd:print wd:value="wd.v('dep').row" />@<wd:print wd:value="wd.v('dep').collection" />
            </wd:for>
        </wd:for></wd:result>
    </wd:session></wd>"#,"").unwrap();
    println!("{}", std::str::from_utf8(r.body()).unwrap());

    let update_xml = r#"<wd><wd:session name="logintest" clear_on_close="true"></wd:session></wd>"#;
    wd.run(update_xml, "").unwrap();
    /*
    let r=wd.run(r#"<wd>
        <wd:script>
            console.log(wd);
        </wd:script>
    </wd>"#,"").unwrap();
    return ;
     */
    //update data.
    /*wd.run(r#"<wd><wd:session name="hoge">
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

    let update_xml = r#"<wd><wd:session name="hoge">
    <wd:update commit="1">
        <collection name="person">
            <field name="name"><wd:print wd:value="wd.input.name" /></field>
            <field name="country"><wd:print wd:value="wd.input.from" /></field>
        </collection>
    </wd:update>
    </wd:session></wd>"#;
    wd.run(
        update_xml,
        r#"{
        "name":"Noah"
        ,"from":"US"
    }"#,
    )
    .unwrap();
    wd.run(
        update_xml,
        r#"{
        "name":"Liam"
        ,"from":"US"
    }"#,
    )
    .unwrap();
    wd.run(
        update_xml,
        r#"{
        "name":"Olivia"
        ,"from":"UK"
    }"#,
    )
    .unwrap();

    //select data.
    let r=wd.run(r#"<wd>
        <wd:search name="p" collection="person">
        </wd:search>
        <wd:result var="q" search="p">
            <div>
                find <wd:print wd:value="wd.v('q').length" /> persons.
            </div>
            <ul>
                <wd:for var="r" index="i" wd:in="wd.v('q')"><li>
                    <wd:print wd:value="wd.v('r').row" /> : <wd:print wd:value="wd.v('r').activity" /> : <wd:print wd:value="wd.v('r').uuid" /> : <wd:print wd:value="wd.v('r').field('name')" /> : <wd:print wd:value="wd.v('r').field('country')" />
                </li></wd:for>
            </ul>
        </wd:result>
        <input type="text" name="hoge" />
        <wd:include src="body.xml" />
    </wd>"#,"").unwrap();
    println!("{}", std::str::from_utf8(r.body()).unwrap());

    //seaech data
    let r=wd.run(r#"<wd>
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
    </wd>"#,"").unwrap();
    println!("{}", std::str::from_utf8(r.body()).unwrap());

    //use javascript
    let r=wd.run(r#"<wd>
        <wd:script>
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
            let hoge=wd.get_contents('body.xml');
            console.log("hoge",hoge);
        </wd:script>
        <wd:search name="p" collection="person">
            <field name="country" method="match" wd:value="wd.general.uk" />
        </wd:search>
        <wd:result var="q" search="p">
            <div>
                <wd:print wd:value="wd.general.ymd()" />
            </div>
            <div>
                find <wd:print wd:value="wd.v('q').length" /> persons from the <wd:print wd:value="wd.general.uk" />.
            </div>
            <ul>
                <wd:for var="r" index="i" wd:in="wd.v('q')"><li>
                    <wd:print wd:value="wd.v('r').row" /> : <wd:print wd:value="wd.v('r').field('name')" /> : <wd:print wd:value="wd.v('r').field('country')" />
                </li></wd:for>
            </ul>
        </wd:result>
    </wd>"#,"").unwrap();
    println!(
        "{} : {}",
        std::str::from_utf8(r.body()).unwrap(),
        r.options_json()
    );

    //search in update section.
    wd.run(r#"<wd><wd:session name="hoge">
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
    </wd:session></wd>"#,"").unwrap();
    let r=wd.run(r#"<wd>
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
    </wd>"#,"").unwrap();
    println!("{}", std::str::from_utf8(r.body()).unwrap());

    //use WebAPI
    let r = wd
        .run(
            r#"<wd>
        <wd:script>
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
        </wd:script>
        a:<wd:print wd:value="wd.general.a" />
        v:<wd:print wd:value="wd.v('a')" />
        input:<wd:print wd:value="wd.input.name" />
        <wd:script>
            wd.stack.pop();
            wd.general.a="OK2";
            wd.general.b=1>2;
        </wd:script>
        a:<wd:print wd:value="wd.general.a" />
        v:<wd:print wd:value="wd.general.b" />
    </wd>"#,
            r#"{
        "name":"Ken"
        ,"from":"US"
    }"#,
        )
        .unwrap();
    println!(
        "{} : {}",
        std::str::from_utf8(r.body()).unwrap(),
        r.options_json()
    );
}
