#[cfg(test)]

#[test]
fn it_works(){
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
    wd.exec(r#"<wd><wd:session name="hoge">
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
    </wd:session></wd>"#,b"").unwrap();

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
    println!("{}",r);

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
    println!("{}",r);

    //use javascript
    let r=wd.exec(r#"<wd>
        <wd:script>
            const ymd=function(){
                const now=new Date();
                return now.getFullYear()+"-"+(now.getMonth()+1)+"-"+now.getDate();
            };
            const uk="UK";
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
    println!("{}",r);

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
    println!("{}",r);

    /*

    //search in update section.
    wd.exec(r#"<wd><wd:session name="hoge">
        <wd:update commit="1">
            <collection name="fields">
                <field name="name">birthday</field>
                <field name="default">1970-01-01</field>
            </collection>
            <collection name="fields">
                <field name="name">country</field>
                <field name="default">US</field>
            </collection>
            <collection name="fields">
                <field name="name">height</field>
                <field name="default">170</field>
            </collection>
        </wd:update>
    </wd:session></wd>"#);
        */
    return;
    
    use chrono::TimeZone;
    let now=chrono::Local.timestamp_opt(chrono::Local::now().timestamp()-1000,0).unwrap().format("%Y-%m-%d %H:%M:%S").to_string();
    let end=chrono::Local.timestamp_opt(chrono::Local::now().timestamp()-100,0).unwrap().format("%Y-%m-%d %H:%M:%S").to_string();

    wd.exec(r#"<wd><wd:session name="hoge" initialize="true">
        <wd:update commit="1">
            <collection name="test">
                <field name="num" type="numeric">3</field>
            </collection>
            <collection name="test">
                <field name="num" type="numeric">2</field>
            </collection>
            <collection name="test">
                <field name="num" type="numeric">3</field>
            </collection>
            <collection name="test">
                <field name="num" type="numeric">3</field>
            </collection>
            <collection name="test">
                <field name="num" type="numeric">4</field>
            </collection>
            <collection name="test">
                <field name="num" type="numeric">7</field>
            </collection>
            <collection name="test">
                <field name="num" type="numeric">10</field>
            </collection>
            <collection name="test">
                <field name="num" type="numeric">11</field>
            </collection>
            <collection name="test">
                <field name="num" type="numeric">20</field>
            </collection>
        </wd:update>
    </wd:session></wd>"#,b"").unwrap();
    
    wd.exec(&(r#"<wd><wd:session name="hoge" initialize="true">
        <wd:update>
            <collection name="sys_ac" row="0" term_begin=""#.to_owned()+&now+r#"" term_end=""#+&end+r#"" activity="active" priority="0">
                <field name="name" type="text">aa</field>
                <field name="num" type="numeric">1</field>
            </collection>
            <collection name="sys_ac" row="0" term_begin=""#+&now+r#"" priority="10">
                <field name="name" type="text">bbb</field>
                <field name="num" type="numeric">2</field>
                <pends key="hoge">
                    <collection name="child" row="0" priority="10">
                        <field name="hoge" type="text">hage</field>
                    </collection>
                </pends>
            </collection>
        </wd:update>
    </wd:session></wd>"#),b"").unwrap();
    
    wd.exec(&(r#"<wd><wd:session name="hoge">
        <wd:update>
            <collection name="sys_ac">
                <field name="name" type="text">TEST</field>
                <field name="num" type="numeric">2</field>
            </collection>
        </wd:update>
    </wd:session></wd>"#),b"").unwrap();

    wd.exec(&(r#"<wd><wd:session name="hoge">
        <wd:update>
            <collection name="sys_ac" term_begin=""#.to_owned()+&now+r#"" row="-1">
                <field name="name" type="text">AA</field>
            </collection>
        </wd:update>
    </wd:session></wd>"#),b"").unwrap();
    
    
    wd.exec(&(r#"<wd><wd:session name="hoge">
        <wd:update>
            <collection name="sys_ac" term_begin=""#.to_owned()+&now+r#"" row="0">
                <field name="name" type="text">cccc</field>
                <field name="num" type="numeric">3</field>
            </collection>
        </wd:update>
    </session></wd>"#),b"").unwrap();
    
    wd.exec(r#"<wd><wd:session name="hoge">
        <wd:update commit="1"></wd:update>
    </wd:session></wd>"#,b"").unwrap();

    let r=wd.exec(&(r#"<wd>
        <wd:script>
            const hoge='HOGE';
            const f=function(){
                return 'FUGA';
            };
        </wd:script>
        <wd:stack var="hoge:2">hoge=<wd:print wd:value="wd.v('hoge')" /><wd:stack var="hoge2:3">
            hoge=<wd:print wd:value="wd.v('hoge')" />
            hoge2=<wd:print wd:value="wd.v('hoge2')" />
            <wd:search name="test" collection="test">
                <field name="num" method="range" value="4..10" />
                <row method="range" value="6..8" />
            </wd:search>
            <wd:result var="q" search="test">
                (TEST)データが<wd:print wd:value="wd.v('q').length" />件あります
                <ul>
                    <wd:for var="r" index="i" wd:in="wd.v('q')"><li>
                        <wd:print wd:value="wd.v('r').row" /> : <wd:print wd:value="wd.v('r').field('num')" />
                    </li></wd:for>
                </ul>
            </wd:result>
        </wd:stack></wd:stack>
    </wd>"#),b"").unwrap();
    println!("{}",r);
    let r=wd.exec(&(r#"<wd>
        <wd:script>
            const hoge='HOGE';
            const f=function(){
                return 'FUGA';
            };
        </wd:script>
        <wd:stack var="hoge:2">hoge=<wd:print wd:value="wd.v('hoge')" /><wd:stack var="hoge2:3">
            hoge=<wd:print wd:value="wd.v('hoge')" />
            hoge2=<wd:print wd:value="wd.v('hoge2')" />
            <wd:search name="s"
                collection="sys_ac"
                activity="active"
                term="in@"#.to_owned()+&chrono::Local.timestamp_opt(chrono::Local::now().timestamp(),0).unwrap().format("%Y-%m-%d %H:%M:%S").to_string()+r#""
            >
                <field name="num" method="match" value="2" />
            </wd:search>
            <wd:search name="test" collection="test">
                <field name="num" method="range" value="4..10" />
                <row method="range" value="6..8" />
                <depend key="" collection="collection_name" row="1" />
                <narrow></narrow>
                <wide></wide>
            </wd:search>
            <wd:result var="q" search="test">
                (TEST)データが<wd:print wd:value="wd.v('q').length" />件あります
                <ul>
                    <wd:for var="r" index="i" wd:in="wd.v('q')"><li>
                        <wd:print wd:value="wd.v('r').row" /> : <wd:print wd:value="wd.v('r').field('num')" />
                    </li></wd:for>
                </ul>
            </wd:result>
            <wd:result var="q" search="s">
                データが<span wd:collection="'hoge'+wd.v('q').length"><wd:print wd:value="wd.v('q').length" /></span>件あります
                <ul>
                    <wd:for var="r" index="i" wd:in="wd.v('q')"><li>
                        <wd:print wd:value="wd.v('i')+1" /> row:<wd:print wd:value="wd.v('r').row" /> : <wd:print wd:value="wd.v('r').field('_uuid')" /> : <wd:print wd:value="wd.v('r').field('name')" /> : <wd:print value="wd.v('r').field('num')" />
                    </li></wd:for>
                    <wd:for var="r" index="i" wd:in="[0,3,1]"><li>
                        OK<wd:print wd:value="wd.v('i')+':'+wd.v('r')" />
                    </li></wd:for>
                </ul>
                hoge=<wd:print wd:value="hoge" />
                <wd:case value="hoge">
                    <wd:when value="2">
                        hogeは2です。
                    </wd:when>
                    <wd:when value="'HOGE'">
                        OKです。
                    </wd:when>
                    <wd:else>
                        else
                    </wd:else>
                </wd:case>
            </wd:result>
        </wd:stack></wd:stack>
        <wd:include src="body.xml" />
    </wd>"#),b"").unwrap();
    println!("{}",r);
    
    return;
    wd.exec(r#"<wd><wd:session="hoge">
        <wd:update commit="1">
            <collection name="sys_ac" row="2">
                <field name="name" type="text">test_rename2</field>
            </collection>
        </wd:update>
    </wd:session></wd>"#,b""
    ).unwrap();

    wd.exec(r#"<wd><wd:session="hoge">
        <wd:update commit="1">
            <collection name="sys_ac" row="2">
                <field name="name" type="text">test_rename3</field>
            </collection>
        </wd:update>
    </wd:session></wd>"#,b""
    ).unwrap();

    wd.exec(r#"<wd><wd:session="hoge">
        <wd:update commit="1">
            <collection name="sys_ac" row="3" activity="inactive">
                <field name="name" type="text">test_rename4</field>
            </collection>
        </wd:update>
    </wd:session></wd>"#,b""
    ).unwrap();

    wd.exec(r#"<wd><wd:session="hoge">
        <wd:select>
            <wd:stack var="hoge:true">
                <wd:script>
                    var hoge="HOGE";
                    var f=function(){
                        return "FUGA"
                    }
                </wd:script>
                hoge=<wd:print wd:value="f()" />
                <wd:query>
                    <wd:search name="s"
                        collection="sys_ac"
                        activity="all"
                    ></wd:search>
                    <wd:result var="q" search="s"><div class="hoge2">
                        データが<span wd:collection="'hoge'+(wd.v('q').length)"><wd:print wd:value="wd.v('q').length" /></span>件あります
                        <ul>
                            <wd:for var="r" index="i" wd:in="wd.v('q')"><li>
                                <wd:print wd:value="wd.v('i')+1" /> row:<wd:print wd:value="wd.v('r').row" /> <wd:print wd:value="wd.v('r').field('_activity')+','+wd.v('r').field('name')" />
                            </li></wd:for>
                            <wd:for var="r" index="i" wd:in="[0,3,1]"><li>
                                OK<wd:print wd:value="wd.v('i')+':'+wd.v('r')" />
                            </li></wd:for>
                        </ul>
                        <wd:case value="hoge">
                            <wd:when value="1">
                                hogeは1です。
                            </wd:when>
                            <wd:when value="'HOGE'">
                                OKです。
                            </wd:when>
                            <wd:else>
                                else
                            </wd:else>
                        </wd:case>
                        hoge=<wd:print wd:value="wd.v('hoge')" />
                    </div></wd:result>
                </wd:query>
            </wd:stack>
            <wd:include src="'hoge.ygl'" />
        </wd:select>
    </wd:session></wd>"#,b""
    ).unwrap();
    
}
