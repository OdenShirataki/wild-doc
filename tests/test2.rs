#[cfg(test)]
#[test]
fn test2() {
    let tokens = xmlparser::Tokenizer::from(
        r#"<wd><wd:search name="exists_admin_account" collection="admin"></wd:search><wd:result var="exists_admin_account" search="exists_admin_account"><wd:case
        wd:value="wd.v('exists_admin_account')!==void 0 &amp;&amp; wd.v('exists_admin_account').length>0"
    ><wd:when
        wd:value="false"
    ><wd:include wd:src="wd.mod_path('db','layout/html.html')"
        var="content_path:wd.mod_path('admin','create_1st.html'),addtional_css:['/-mod/admin/assets/css/create_1st.css']"
    /></wd:when><wd:else><wd:stack
        var="current_session:'login'"
    ><wd:session
        wd:name="wd.session_name(wd.v('current_session'))"
    ><wd:search name="login" collection="login"></wd:search><wd:result
        var="login" search="login"
    ><wd:case wd:value="wd.v('login')!==void 0 &amp;&amp; wd.v('login').length>0"><wd:when
        wd:value="true"
    ><wd:script>
        const split=wd.input.path.split('/');
        const mod=split[2];
        if(mod!=""){
            wd.general.db_contents_path=wd.mod_path(mod,'db/index.html');
            wd.general.db_include_head=wd.mod_path(mod,'db/head.html');
        }else{
            wd.general.db_contents_path=wd.mod_path('db',"index.html");
            wd.general.db_include_head=void 0;
        }
    </wd:script><wd:include wd:src="wd.mod_path('db','layout/html.html')" /></wd:when><wd:else><wd:include
        wd:src="wd.mod_path('db','layout/html.html')"
        var="content_path:wd.mod_path('login','form.html'),addtional_css:['/-mod/login/assets/css/login.css']"
    /></wd:else></wd:case></wd:result></wd:session></wd:stack></wd:else></wd:case></wd>"#,
    );
    for i in tokens {
        println!("{:#?}", i);
    }
}
