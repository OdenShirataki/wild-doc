# semilattice-script

## Example

```rust
use chrono::TimeZone;
use semilattice_script::*;

let dir="./ss-test/";
if std::path::Path::new(dir).exists(){
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::create_dir_all(dir).unwrap();
}else{
    std::fs::create_dir_all(dir).unwrap();
}
let mut ss=SemilatticeScript::new(
    dir
    ,IncludeLocal::new("./include/")
).unwrap();

//update data.
ss.exec(r#"<ss><ss:session name="hoge">
    <ss:update commit="1">
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
    </ss:update>
</ss:session></ss>"#);

//select data.
let r=ss.exec(r#"<ss>
    <ss:search name="p" collection="person">
    </ss:search>
    <ss:result var="q" search="p">
        <div>
            find <ss:print ss:value="ss.v('q').length" /> persons.
        </div>
        <ul>
            <ss:for var="r" index="i" ss:in="ss.v('q')"><li>
                <ss:print ss:value="ss.v('r').row" /> : <ss:print ss:value="ss.v('r').field('name')" /> : <ss:print ss:value="ss.v('r').field('country')" />
            </li></ss:for>
        </ul>
    </ss:result>
</ss>"#);
println!("{}",r);

//seaech data
let r=ss.exec(r#"<ss>
    <ss:search name="p" collection="person">
        <field name="country" method="match" value="US" />
    </ss:search>
    <ss:result var="q" search="p">
        <div>
            find <ss:print ss:value="ss.v('q').length" /> persons from the US.
        </div>
        <ul>
            <ss:for var="r" index="i" ss:in="ss.v('q')"><li>
                <ss:print ss:value="ss.v('r').row" /> : <ss:print ss:value="ss.v('r').field('name')" /> : <ss:print ss:value="ss.v('r').field('country')" />
            </li></ss:for>
        </ul>
        <ss:include src="'hoge.xml'" />
    </ss:result>
</ss>"#);
println!("{}",r);

//use javascript
let r=ss.exec(r#"<ss>
    <ss:script>
        const ymd=function(){
            const now=new Date();
            return now.getFullYear()+"-"+(now.getMonth()+1)+"-"+now.getDate();
        };
        const uk="UK";
    </ss:script>
    <ss:search name="p" collection="person">
        <field name="country" method="match" ss:value="uk" />
    </ss:search>
    <ss:result var="q" search="p">
        <div>
            <ss:print ss:value="ymd()" />
        </div>
        <div>
            find <ss:print ss:value="ss.v('q').length" /> persons from the <ss:print ss:value="uk" />.
        </div>
        <ul>
            <ss:for var="r" index="i" ss:in="ss.v('q')"><li>
                <ss:print ss:value="ss.v('r').row" /> : <ss:print ss:value="ss.v('r').field('name')" /> : <ss:print ss:value="ss.v('r').field('country')" />
            </li></ss:for>
        </ul>
    </ss:result>
</ss>"#);
println!("{}",r);

//search in update section.
ss.exec(r#"<ss><ss:session name="hoge">
    <ss:update commit="1">
        <ss:search name="person" collection="person"></ss:search>
        <ss:result var="q" search="person">
            <ss:for var="r" index="i" ss:in="ss.v('q')">
                hoge:<ss:print ss:value="ss.v('r').row" />
                <collection name="person" ss:row="ss.v('r').row">
                    <field name="name">Renamed <ss:print ss:value="ss.v('r').field('name')" /></field>
                    <field name="country"><ss:print ss:value="ss.v('r').field('country')" /></field>
                </collection>
            </ss:for>
        </ss:result>
    </ss:update>
</ss:session></ss>"#);
let r=ss.exec(r#"<ss>
    <ss:search name="p" collection="person"></ss:search>
    <ss:result var="q" search="p">
        <div>
            find <ss:print ss:value="ss.v('q').length" /> persons.
        </div>
        <ul>
            <ss:for var="r" index="i" ss:in="ss.v('q')"><li>
                <ss:print ss:value="ss.v('r').row" /> : <ss:print ss:value="ss.v('r').field('name')" /> : <ss:print ss:value="ss.v('r').field('country')" />
            </li></ss:for>
        </ul>
    </ss:result>
</ss>"#);
println!("{}",r);

```