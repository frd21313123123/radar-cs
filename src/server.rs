//! Minimal bounded local HTTP server; requests carry a random bearer token.
use std::{io::{Read,Write},net::{TcpListener,TcpStream,SocketAddr},path::PathBuf,sync::{Arc,Mutex},time::Duration};
use serde_json::{Value,json};
use crate::{assets,icons,gsi,state::SharedState,overlay::OverlayController};

pub struct HttpServer {
    pub bind:String,pub port:u16,pub token:String,pub gsi_token:String,
    pub cache:PathBuf,pub offline:bool,pub html:PathBuf,pub state:Arc<Mutex<SharedState>>,pub overlay:OverlayController,
}
impl HttpServer {
    pub fn serve(self)->Result<(),String>{
        let listener=TcpListener::bind((self.bind.as_str(),self.port)).map_err(|e|format!("Bind failed: {e}"))?;
        self.serve_listener(listener)
    }
    pub fn serve_listener(self,listener:TcpListener)->Result<(),String>{
        let server=Arc::new(self);
        for conn in listener.incoming(){match conn{
            Ok(stream)=>{let ctx=server.clone();std::thread::spawn(move||{
                let _=stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _=stream.set_write_timeout(Some(Duration::from_secs(5)));
                let _=handle(stream,&ctx);
            });}
            Err(e)=>eprintln!("[http] accept: {e}"),
        }}Ok(())
    }
}
fn respond(stream:&mut TcpStream,status:u16,body:&[u8],ctype:&str)->std::io::Result<()> {
    respond_cached(stream,status,body,ctype,"no-store")
}
fn respond_cached(stream:&mut TcpStream,status:u16,body:&[u8],ctype:&str,cache:&str)->std::io::Result<()> {
    let status_text=match status{200=>"OK",400=>"Bad Request",403=>"Forbidden",404=>"Not Found",413=>"Payload Too Large",502=>"Bad Gateway",_=>"Error"};
    let header=format!("HTTP/1.1 {status} {status_text}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: {cache}\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'\r\n\r\n",body.len());
    stream.write_all(header.as_bytes())?;stream.write_all(body)
}
fn reply_json(stream:&mut TcpStream,status:u16,v:Value)->std::io::Result<()>{let bytes=v.to_string();respond(stream,status,bytes.as_bytes(),"application/json; charset=utf-8")}
fn query_arg<'a>(query:&'a str,name:&str)->Option<&'a str>{query.split('&').filter_map(|arg|arg.split_once('=')).find(|(k,_)|*k==name).map(|(_,v)|v)}

// Decode a bounded UTF-8 query value, rejecting malformed escapes.
fn query_value(query:&str,name:&str)->Option<String>{
    let raw=query_arg(query,name)?;
    if raw.len()>768{return None;}
    let mut bytes=Vec::new();let mut input=raw.bytes();
    while let Some(b)=input.next(){
        bytes.push(match b{
            b'%' => {let hi=(input.next()? as char).to_digit(16)?;let lo=(input.next()? as char).to_digit(16)?;(hi*16+lo) as u8},
            b'+' => b' ',_=>b,
        });
    }
    String::from_utf8(bytes).ok()
}

fn handle(mut stream:TcpStream,server:&HttpServer)->std::io::Result<()> {
    let peer:SocketAddr=stream.peer_addr()?;
    let mut header=Vec::new();let mut buf=[0_u8;4096];let boundary;
    loop {
        if header.len()>16*1024{return reply_json(&mut stream,413,json!({"error":"headers too large"}));}
        let size=stream.read(&mut buf)?;if size==0{return Ok(());}header.extend_from_slice(&buf[..size]);
        if let Some(pos)=header.windows(4).position(|x|x==b"\r\n\r\n"){boundary=pos+4;break;}
    }
    let head=match std::str::from_utf8(&header[..boundary]){Ok(v)=>v,Err(_)=>return reply_json(&mut stream,400,json!({"error":"bad headers"}))};
    let mut lines=head.split("\r\n");let first=lines.next().unwrap_or("");let parts:Vec<&str>=first.split_whitespace().collect();
    if parts.len()!=3{return reply_json(&mut stream,400,json!({"error":"bad request line"}));}
    let method=parts[0];let target=parts[1];
    let (path,query)=target.split_once('?').unwrap_or((target,""));
    if method=="POST" && path=="/gsi" {
        if !peer.ip().is_loopback(){return reply_json(&mut stream,403,json!({"error":"GSI only allowed from localhost"}));}
        let content=lines.filter_map(|l|l.split_once(':')).find(|(name,_)|name.eq_ignore_ascii_case("Content-Length"))
            .and_then(|(_,v)|v.trim().parse::<usize>().ok());
        let len=match content{Some(n) if n<=32768=>n,_=>return reply_json(&mut stream,413,json!({"error":"missing/large body"}))};
        let mut body=header[boundary..].to_vec();
        while body.len()<len {
            let r=stream.read(&mut buf)?;if r==0{break;}body.extend_from_slice(&buf[..r]);
            if body.len()>32768{return reply_json(&mut stream,413,json!({"error":"body too large"}));}
        }
        if body.len()<len{return reply_json(&mut stream,400,json!({"error":"incomplete GSI payload"}));}
        let value:Value=match serde_json::from_slice(&body[..len]){Ok(v)=>v,Err(_)=>return reply_json(&mut stream,400,json!({"error":"invalid JSON"}))};
        let token=value.get("auth").and_then(|v|v.get("token")).and_then(Value::as_str).unwrap_or("");
        if !gsi::token_equal(token,&server.gsi_token){return reply_json(&mut stream,403,json!({"error":"GSI token invalid"}));}
        if let Ok(mut state)=server.state.lock(){
            // An explicit map removal is authoritative; quiet/missing fields
            // retain the last map, and manual selection remains independent.
            if value.get("map")==Some(&Value::Null){state.clear_automatic_map();}
            else if let Some(name)=gsi::gsi_map(&value){state.set_map(&name,"gsi");}
        }
        return reply_json(&mut stream,200,json!({"ok":true}));
    }
    // Main and native maps share one explicit selection. JSON + token keeps
    // this mutation within the authenticated same-origin UI, just like overlay.
    if method=="POST" && path=="/api/map-selection" {
        if query_arg(query,"token")!=Some(server.token.as_str()){
            return reply_json(&mut stream,403,json!({"error":"invalid access token"}));
        }
        let headers:Vec<(&str,&str)>=lines.filter_map(|l|l.split_once(':')).collect();
        let ctype=headers.iter().find(|(k,_)|k.eq_ignore_ascii_case("Content-Type")).map(|(_,v)|v.trim()).unwrap_or("");
        if !ctype.starts_with("application/json"){return reply_json(&mut stream,400,json!({"error":"expected application/json"}));}
        let len=headers.iter().find(|(k,_)|k.eq_ignore_ascii_case("Content-Length")).and_then(|(_,v)|v.trim().parse::<usize>().ok());
        let len=match len{Some(n) if n>0&&n<=512=>n,_=>return reply_json(&mut stream,413,json!({"error":"invalid selection size"}))};
        let mut body=header[boundary..].to_vec();
        while body.len()<len{
            let count=stream.read(&mut buf)?;if count==0{break;}
            body.extend_from_slice(&buf[..count]);
            if body.len()>512{return reply_json(&mut stream,413,json!({"error":"selection too large"}));}
        }
        if body.len()<len{return reply_json(&mut stream,400,json!({"error":"incomplete selection"}));}
        let value:Value=match serde_json::from_slice(&body[..len]){Ok(v)=>v,Err(_)=>return reply_json(&mut stream,400,json!({"error":"invalid JSON"}))};
        let selected=match value.get("map"){Some(Value::Null)=>None,Some(Value::String(s))=>Some(s.as_str()),_=>return reply_json(&mut stream,400,json!({"error":"map must be a name or null"}))};
        let mut state=server.state.lock().map_err(|_|std::io::Error::other("poisoned state"))?;
        if let Err(error)=state.set_manual_map(selected){return reply_json(&mut stream,400,json!({"error":error}));}
        let snapshot=state.snapshot();
        return reply_json(&mut stream,200,json!({"game_map":snapshot.game_map,"game_map_source":snapshot.game_map_source}));
    }
    // Only the authenticated CAS page can control the native Win32 window.
    // Application/json causes a cross-origin preflight; CORS is not enabled.
    if method=="POST" && path=="/api/overlay" {
        if query_arg(query,"token")!=Some(server.token.as_str()) {
            return reply_json(&mut stream,403,json!({"error":"invalid access token"}));
        }
        let headers:Vec<(&str,&str)>=lines.filter_map(|l|l.split_once(':')).collect();
        let content_type=headers.iter().find(|(k,_)|k.eq_ignore_ascii_case("Content-Type"))
            .map(|(_,v)|v.trim()).unwrap_or("");
        if !content_type.starts_with("application/json") {
            return reply_json(&mut stream,400,json!({"error":"expected application/json"}));
        }
        let len=headers.iter().find(|(k,_)|k.eq_ignore_ascii_case("Content-Length"))
            .and_then(|(_,v)|v.trim().parse::<usize>().ok());
        let len=match len {Some(n) if n>0 && n<=4096=>n,_=>return reply_json(&mut stream,413,json!({"error":"invalid overlay payload size"}))};
        let mut body=header[boundary..].to_vec();
        while body.len()<len {
            let count=stream.read(&mut buf)?;if count==0{break;}
            body.extend_from_slice(&buf[..count]);
            if body.len()>4096{return reply_json(&mut stream,413,json!({"error":"payload too large"}));}
        }
        if body.len()<len {return reply_json(&mut stream,400,json!({"error":"incomplete payload"}));}
        let v:Value=match serde_json::from_slice(&body[..len]){Ok(v)=>v,Err(_)=>return reply_json(&mut stream,400,json!({"error":"invalid JSON"}))};
        return match server.overlay.update(&v) {
            Ok(v)=>reply_json(&mut stream,200,v),
            Err(e)=>reply_json(&mut stream,400,json!({"error":e})),
        };
    }
    if method!="GET" {return reply_json(&mut stream,404,json!({"error":"not found"}));}
    if query_arg(query,"token")!=Some(server.token.as_str()){
        if path=="/"{
            return respond(&mut stream,403,include_bytes!("../web/access.html"),"text/html; charset=utf-8");
        }
        return reply_json(&mut stream,403,json!({"error":"invalid access token"}));
    }
    match path {
        "/"=>{
            // The EXE and its browser UI must always be the *same* release.
            // An old unpacked web/index.html can silently break maps and
            // new API features when the user replaces only the EXE.
            let embedded=include_bytes!("../web/index.html");
            if std::env::var_os("CAS_RADAR_DEV_WEB").as_deref()==Some(std::ffi::OsStr::new("1")){
                let bytes=std::fs::read(&server.html).unwrap_or_else(|_|embedded.to_vec());
                respond(&mut stream,200,&bytes,"text/html; charset=utf-8")
            }else{
                respond(&mut stream,200,embedded,"text/html; charset=utf-8")
            }
        }
        "/api/state"=>{
            let snap=server.state.lock().map(|s|s.snapshot()).map_err(|_|std::io::Error::other("poisoned state"))?;
            reply_json(&mut stream,200,serde_json::to_value(snap).unwrap_or(json!({"status":"error"})))
        }
        "/api/weapon-icon"=>{
            let name=query_value(query,"weapon").unwrap_or_default();
            respond_cached(&mut stream,200,icons::weapon_icon(&name).as_bytes(),"image/svg+xml","private, max-age=86400")
        }
        "/api/agent-icon"=>{
            let model=query_value(query,"model").unwrap_or_default();
            match icons::agent_icon(&model,&server.cache,server.offline){
                Some(bytes)=>respond_cached(&mut stream,200,&bytes,"image/png","private, max-age=86400"),
                None=>respond_cached(&mut stream,200,icons::NEUTRAL_AGENT.as_bytes(),"image/svg+xml","private, max-age=30"),
            }
        }
        "/api/overlay"=>reply_json(&mut stream,200,server.overlay.status()),
        "/api/bomb-icon"=>respond_cached(&mut stream,200,include_bytes!("../assets/bomb_ground.png"),"image/png","private, max-age=86400"),
        "/api/maps"=>{
            let maps=server.state.lock().map(|s|s.maps.clone()).map_err(|_|std::io::Error::other("poisoned state"))?;
            reply_json(&mut stream,200,maps)
        }
        "/api/map-image"=>{
            let map=query_arg(query,"map").unwrap_or("");let layer=query_arg(query,"layer").unwrap_or("default");
            let maps=server.state.lock().map(|s|s.maps.clone()).map_err(|_|std::io::Error::other("poisoned state"))?;
            match assets::map_image(map,layer,&maps,&server.cache,server.offline){
                Ok(bytes)=>respond(&mut stream,200,&bytes,"image/png"),
                Err(e)=>reply_json(&mut stream,502,json!({"error":e})),
            }
        }
        _=>reply_json(&mut stream,404,json!({"error":"not found"})),
    }
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn bundled_web_matches_timer_and_map(){
        let html=include_str!("../web/index.html");
        assert!(html.contains("function renderBackground()"));
        assert!(html.contains("bombTimerPanel"));
        assert!(!html.contains("bombTimerHp"));
    }
    #[test]fn bomb_png_embedded(){assert!(include_bytes!("../assets/bomb_ground.png").starts_with(b"\x89PNG\r\n\x1a\n"));}
    #[test]fn decode_icon_query(){
        assert_eq!(query_value("weapon=AK-47","weapon").as_deref(),Some("AK-47"));
        assert_eq!(query_value("weapon=HE%20Grenade","weapon").as_deref(),Some("HE Grenade"));
        assert_eq!(query_value("weapon=M4A1%2DS","weapon").as_deref(),Some("M4A1-S"));
        assert_eq!(query_value("weapon=%GG","weapon"),None);
        assert_eq!(query_value("weapon=%","weapon"),None);
    }
    #[test]fn auth_query(){assert_eq!(query_arg("map=de_nuke&token=hello","token"),Some("hello"));assert_eq!(query_arg("token=bad","token"),Some("bad"));}
}
