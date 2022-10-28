# semilattice-script

## Example

```rust
use semilattice_script::*;

let dir="./ss-test/";
if std::path::Path::new(dir).exists(){
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::create_dir_all(dir).unwrap();
}else{
    std::fs::create_dir_all(dir).unwrap();
}

let mut ss=SemilatticeScript::new(dir).unwrap();

let mut ss=SemilatticeScript::new(dir).unwrap();

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
            find <ss:print value="ss.v('q').length" /> persons.
        </div>
        <ul>
            <ss:for var="r" index="i" in="ss.v('q')"><li>
                <ss:print value="ss.v('r').row" /> : <ss:print value="ss.v('r').field('name')" /> : <ss:print value="ss.v('r').field('country')" />
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
            find <ss:print value="ss.v('q').length" /> persons from the US.
        </div>
        <ul>
            <ss:for var="r" index="i" in="ss.v('q')"><li>
                <ss:print value="ss.v('r').row" /> : <ss:print value="ss.v('r').field('name')" /> : <ss:print value="ss.v('r').field('country')" />
            </li></ss:for>
        </ul>
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
    </ss:script>
    <ss:search name="p" collection="person">
        <field name="country" method="match" value="US" />
    </ss:search>
    <ss:result var="q" search="p">
        <div>
            <ss:print value="ymd()" />
        </div>
        <div>
            find <ss:print value="ss.v('q').length" /> persons from the US.
        </div>
        <ul>
            <ss:for var="r" index="i" in="ss.v('q')"><li>
                <ss:print value="ss.v('r').row" /> : <ss:print value="ss.v('r').field('name')" /> : <ss:print value="ss.v('r').field('country')" />
            </li></ss:for>
        </ul>
    </ss:result>
</ss>"#);
println!("{}",r);

```