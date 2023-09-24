use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
};

use hashbrown::HashMap;

pub struct WildDocResult {
    body: Vec<u8>,
    options_json: String,
}
impl WildDocResult {
    pub fn body(&self) -> &[u8] {
        &self.body
    }
    pub fn options_json(&self) -> &str {
        &self.options_json
    }
}

pub struct WildDocClient {
    document_root: PathBuf,
    sock: TcpStream,
}
impl WildDocClient {
    pub fn new<P: AsRef<Path>>(host: &str, port: &str, document_root: P, dbname: &str) -> Self {
        let mut sock =
            TcpStream::connect(&(host.to_owned() + ":" + port)).expect("failed to connect server");
        sock.set_nonblocking(false).expect("out of service");
        sock.write_all(dbname.as_bytes()).unwrap();
        sock.write_all(&[0]).unwrap();

        let mut sig = Vec::new();
        let mut reader = BufReader::new(&sock);
        reader.read_until(0, &mut sig).unwrap();

        Self {
            document_root: {
                let mut path = document_root.as_ref().to_path_buf();
                path.push(dbname);
                path
            },
            sock,
        }
    }
    pub fn exec(&mut self, xml: &str, input_json: &str) -> std::io::Result<WildDocResult> {
        let mut include_cache = HashMap::new();

        if input_json.len() > 0 {
            self.sock.write_all(input_json.as_bytes())?;
        }
        self.sock.write_all(&[0])?;

        self.sock.write_all(xml.as_bytes())?;
        self.sock.write_all(&[0])?;

        let mut reader = BufReader::new(self.sock.try_clone().unwrap());
        loop {
            let mut recv_include = Vec::new();
            if reader.read_until(0, &mut recv_include)? > 0 {
                if recv_include.starts_with(b"include:") {
                    recv_include.remove(recv_include.len() - 1);
                    let mut exists = false;
                    if let Ok(str) = std::str::from_utf8(&recv_include) {
                        let s: Vec<&str> = str.split("include:/").collect();
                        if s.len() >= 2 {
                            let mut path = self.document_root.clone();
                            path.push(s[1]);
                            if let Some(include_xml) =
                                include_cache.entry(path).or_insert_with_key(|path| {
                                    match std::fs::File::open(path) {
                                        Ok(mut f) => {
                                            let mut contents = Vec::new();
                                            let _ = f.read_to_end(&mut contents);
                                            Some(contents)
                                        }
                                        _ => None,
                                    }
                                })
                            {
                                exists = true;
                                let exists: [u8; 1] = [1];
                                self.sock.write_all(&exists)?;

                                let len = include_xml.len() as u64;
                                self.sock.write_all(&len.to_be_bytes())?;
                                self.sock.write_all(&include_xml)?;
                            }
                        }
                    }
                    if !exists {
                        let exists: [u8; 1] = [0];
                        self.sock.write_all(&exists)?;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let mut len: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
        reader.read_exact(&mut len)?;
        let len = u64::from_be_bytes(len) as usize;

        let mut recv_body = Vec::<u8>::with_capacity(len);
        unsafe {
            recv_body.set_len(len);
        }
        reader.read_exact(recv_body.as_mut_slice())?;

        let mut recv_options = Vec::new();
        reader.read_until(0, &mut recv_options)?;
        recv_options.remove(recv_options.len() - 1);

        Ok(WildDocResult {
            body: recv_body,
            options_json: String::from_utf8(recv_options).unwrap_or("".to_owned()),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let mut client = WildDocClient::new("localhost", "51818", "./test/", "test");
        client
            .exec(
                r#"<wd:session name="hoge">
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
        </wd:session>"#,
                "",
            )
            .unwrap();

        /*
        client.exec(r#"
            include-test:<wd:include src="hoge.xml" />
            <wd:search name="p" collection="person">
            </wd:search>
            OK
            <wd:result var="q" search="p">
                <div>
                    find <wd:print value:var="q.len" /> persons.
                </div>
                <ul>
                    <wd:for var="r" key="i" in:var="q.rows"><li>
                        <wd:print value:var="r.row" /> : <wd:print value:var="r.field.name" /> : <wd:print value:var="r.field.country" />
                    </li></wd:for>
                </ul>
            </wd:result>
        "#);


        client.exec(r#"
            <wd:search name="p" collection="person">
                <field name="country" method="match" value="US" />
            </wd:search>
            <wd:result var="q" search="p">
                <div>
                    find <wd:print value:var="q.len" /> persons from the US.
                </div>
                <ul>
                    <wd:for var="r" key="i" in:var="q.rows"><li>
                        <wd:print value:var="r.row" /> : <wd:print value:var="r.field.name" /> : <wd:print value:var="r.field.country" />
                    </li></wd:for>
                </ul>
            </wd:result>
        "#);
        client.exec(r#"
            <?js
                const ymd=function(){
                    const now=new Date();
                    return now.getFullYear()+"-"+(now.getMonth()+1)+"-"+now.getDate();
                };
                const uk="UK";
            ?>
            <wd:search name="p" collection="person">
                <field name="country" method="match" value="uk" />
            </wd:search>
            <wd:result var="q" search="p">
                <div>
                    <wd:print value:js="ymd()" />
                </div>
                <div>
                    find <wd:print value:var="q.len" /> persons from the <wd:print value="uk" />.
                </div>
                <ul>
                    <wd:for var="r" key="i" in:var="q.rows"><li>
                        <wd:print value:var="r.row" /> : <wd:print value:var="'.field.name" /> : <wd:print value:var="r.field.country" />
                    </li></wd:for>
                </ul>
            </wd:result>
        "#);
        */
        client
            .exec(
                r#"<wd:session name="hoge">
            <wd:update commit="true">
                <wd:search name="person" collection="person"></wd:search>
                <wd:result var="q" search="person">
                    <wd:for var="r" key="i" in:var="q.rows">
                        <collection name="person" row:var="r.row">
                            <field name="name">Renamed <wd:print value:var="r.field.name" /></field>
                            <field name="country"><wd:print value:var="r.field.country" /></field>
                        </collection>
                    </wd:for>
                </wd:result>
            </wd:update>
        </wd:session>"#,
                "",
            )
            .unwrap();
        let r=client.exec(r#"
            <wd:search name="p" collection="person"></wd:search>
            <wd:result var="q" search="p">
                <div>
                    find <wd:print value:var="q.len" /> persons.
                </div>
                <ul>
                    <wd:for var="r" key="i" in:var="q.rows"><li>
                        <wd:print value:var="r.row" /> : <wd:print value:var="'r.field.name" /> : <wd:print value:var="r.field.country" />
                    </li></wd:for>
                </ul>
            </wd:result>
        "#,"").unwrap();
        println!("{}", std::str::from_utf8(&r.body()).unwrap());
    }
}
