#[cfg(test)]

#[test]
fn it_works(){
    use chrono::TimeZone;
    use semilattice_script::*;

    let dir="./ss-test/";
    if std::path::Path::new(dir).exists(){
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::create_dir_all(dir).unwrap();
    }else{
        std::fs::create_dir_all(dir).unwrap();
    }
    let mut ss=SemilatticeScript::new(dir).unwrap();
    {
        let now=chrono::Local.timestamp(chrono::Local::now().timestamp()-1000,0).format("%Y-%m-%d %H:%M:%S").to_string();
        let end=chrono::Local.timestamp(chrono::Local::now().timestamp()-100,0).format("%Y-%m-%d %H:%M:%S").to_string();

        ss.exec(r#"<ss><ss:session name="hoge" initialize="true">
            <ss:update commit="1">
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
            </ss:update>
        </ss:session></ss>"#);
        
        ss.exec(&(r#"<ss><ss:session name="hoge" initialize="true">
            <ss:update>
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
            </ss:update>
        </ss:session></ss>"#));
        
        ss.exec(&(r#"<ss><ss:session name="hoge">
            <ss:update>
                <collection name="sys_ac">
                    <field name="name" type="text">TEST</field>
                    <field name="num" type="numeric">2</field>
                </collection>
            </ss:update>
        </ss:session></ss>"#));

        ss.exec(&(r#"<ss><ss:session name="hoge">
            <ss:update>
                <collection name="sys_ac" term_begin=""#.to_owned()+&now+r#"" row="-1">
                    <field name="name" type="text">AA</field>
                </collection>
            </ss:update>
        </ss:session></ss>"#));
        
        
        ss.exec(&(r#"<ss><ss:session name="hoge">
            <ss:update>
                <collection name="sys_ac" term_begin=""#.to_owned()+&now+r#"" row="0">
                    <field name="name" type="text">cccc</field>
                    <field name="num" type="numeric">3</field>
                </collection>
            </ss:update>
        </session></ss>"#));
        
        ss.exec(r#"<ss><ss:session name="hoge">
            <ss:update commit="1"></ss:update>
        </ss:session></ss>"#);

        let r=ss.exec(&(r#"<ss>
            <ss:script>
                const hoge='HOGE';
                const f=function(){
                    return 'FUGA';
                };
            </ss:script>
            <ss:stack var="hoge:2">hoge=<ss:print value="ss.v('hoge')" /><ss:stack var="hoge2:3">
                hoge=<ss:print value="ss.v('hoge')" />
                hoge2=<ss:print value="ss.v('hoge2')" />
                <ss:search name="test" collection="test">
                    <field name="num" method="range" value="4..10" />
                    <row method="range" value="6..8" />
                </ss:search>
                <ss:result var="q" search="test">
                    (TEST)データが<ss:print value="ss.v('q').length" />件あります
                    <ul>
                        <ss:for var="r" index="i" in="ss.v('q')"><li>
                            <ss:print value="ss.v('r').row" /> : <ss:print value="ss.v('r').field('num')" />
                        </li></ss:for>
                    </ul>
                </ss:result>
            </ss:stack></ss:stack>
        </ss>"#));
        println!("{}",r);
        let r=ss.exec(&(r#"<ss>
            <ss:script>
                const hoge='HOGE';
                const f=function(){
                    return 'FUGA';
                };
            </ss:script>
            <ss:stack var="hoge:2">hoge=<ss:print value="ss.v('hoge')" /><ss:stack var="hoge2:3">
                hoge=<ss:print value="ss.v('hoge')" />
                hoge2=<ss:print value="ss.v('hoge2')" />
                <ss:search name="s"
                    collection="sys_ac"
                    activity="active"
                    term="in@"#.to_owned()+&chrono::Local.timestamp(chrono::Local::now().timestamp(),0).format("%Y-%m-%d %H:%M:%S").to_string()+r#""
                >
                    <field name="num" method="match" value="2" />
                </ss:search>
                <ss:search name="test" collection="test">
                    <field name="num" method="range" value="4..10" />
                    <row method="range" value="6..8" />
                    <depend key="" collection="collection_name" row="1" />
                    <narrow></narrow>
                    <wide></wide>
                </ss:search>
                <ss:result var="q" search="test">
                    (TEST)データが<ss:print value="ss.v('q').length" />件あります
                    <ul>
                        <ss:for var="r" index="i" in="ss.v('q')"><li>
                            <ss:print value="ss.v('r').row" /> : <ss:print value="ss.v('r').field('num')" />
                        </li></ss:for>
                    </ul>
                </ss:result>
                <ss:result var="q" search="s">
                    データが<span ss:collection="'hoge'+ss.v('q').length"><ss:print value="ss.v('q').length" /></span>件あります
                    <ul>
                        <ss:for var="r" index="i" in="ss.v('q')"><li>
                            <ss:print value="ss.v('i')+1" /> row:<ss:print value="ss.v('r').row" /> : <ss:print value="ss.v('r').field('_uuid')" /> : <ss:print value="ss.v('r').field('name')" /> : <ss:print value="ss.v('r').field('num')" />
                        </li></ss:for>
                        <ss:for var="r" index="i" in="[0,3,1]"><li>
                            OK<ss:print value="ss.v('i')+':'+ss.v('r')" />
                        </li></ss:for>
                    </ul>
                    hoge=<ss:print value="hoge" />
                    <ss:case value="hoge">
                        <ss:when value="2">
                            hogeは2です。
                        </ss:when>
                        <ss:when value="'HOGE'">
                            OKです。
                        </ss:when>
                        <ss:else>
                            else
                        </ss:else>
                    </ss:case>
                </ss:result>
            </ss:stack></ss:stack>
            <ss:include src="'hoge.ygl'" />
        </ss>"#));
        println!("{}",r);
        
        return;
        ss.exec(r#"<ss><ss:session="hoge">
            <ss:update commit="1">
                <collection name="sys_ac" row="2">
                    <field name="name" type="text">test_rename2</field>
                </collection>
            </ss:update>
        </ss:session></ss>"#
        );

        ss.exec(r#"<ss><ss:session="hoge">
            <ss:update commit="1">
                <collection name="sys_ac" row="2">
                    <field name="name" type="text">test_rename3</field>
                </collection>
            </ss:update>
        </ss:session></ss>"#
        );

        ss.exec(r#"<ss><ss:session="hoge">
            <ss:update commit="1">
                <collection name="sys_ac" row="3" activity="inactive">
                    <field name="name" type="text">test_rename4</field>
                </collection>
            </ss:update>
        </ss:session></ss>"#
        );

        ss.exec(r#"<ss><ss:session="hoge">
            <ss:select>
                <ss:stack var="hoge:true">
                    <ss:script>
                        var hoge="HOGE";
                        var f=function(){
                            return "FUGA"
                        }
                    </ss:script>
                    hoge=<ss:print value="f()" />
                    <ss:query>
                        <ss:search name="s"
                            collection="sys_ac"
                            activity="all"
                        ></ss:search>
                        <ss:result var="q" search="s"><div class="hoge2">
                            データが<span ss:collection="'hoge'+(ss.v('q').length)"><ss:print value="ss.v('q').length" /></span>件あります
                            <ul>
                                <ss:for var="r" index="i" in="ss.v('q')"><li>
                                    <ss:print value="ss.v('i')+1" /> row:<ss:print value="ss.v('r').row" /> <ss:print value="ss.v('r').field('_activity')+','+ss.v('r').field('name')" />
                                </li></ss:for>
                                <ss:for var="r" index="i" in="[0,3,1]"><li>
                                    OK<ss:print value="ss.v('i')+':'+ss.v('r')" />
                                </li></ss:for>
                            </ul>
                            <ss:case value="hoge">
                                <ss:when value="1">
                                    hogeは1です。
                                </ss:when>
                                <ss:when value="'HOGE'">
                                    OKです。
                                </ss:when>
                                <ss:else>
                                    else
                                </ss:else>
                            </ss:case>
                            hoge=<ss:print value="ss.v('hoge')" />
                        </div></ss:result>
                    </ss:query>
                </ss:stack>
                <ss:include src="'hoge.ygl'" />
            </s:select>
        </ss:session></ss>"#
        );
    }
}
