//! Steam profile avatars, fetched without an API key and served from the local cache.
use std::{collections::HashMap,fs,path::Path,sync::{Mutex,OnceLock},time::{Duration,Instant,SystemTime}};

const MAX_IMAGE:usize=2_000_000;
const CACHE_AGE:Duration=Duration::from_secs(86400);

pub const NEUTRAL_AVATAR:&str=r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" role="img"><title>Аватар Steam недоступен</title><rect width="64" height="64" rx="10" fill="#203144"/><circle cx="32" cy="23" r="12" fill="#a8bac9"/><path d="M10 61v-9c0-22 44-22 44 0v9" fill="#829aaf"/></svg>"##;

/// Public individual accounts only. Bots, zero IDs and other account types
/// have no Steam profile. Keep all 64 bits when passing the ID to JavaScript.
pub(crate) fn steam_id64(value:u64)->Option<String>{
    (value>>32==0x01100001 && value as u32!=0).then(||value.to_string())
}

pub struct Avatar {pub bytes:Vec<u8>,pub content_type:&'static str}
impl Avatar {
    fn from_bytes(bytes:Vec<u8>)->Option<Self>{
        if bytes.len()>MAX_IMAGE{return None;}
        let content_type=if bytes.starts_with(b"\xff\xd8\xff")&&bytes.ends_with(b"\xff\xd9"){"image/jpeg"}
            else if bytes.starts_with(b"\x89PNG\r\n\x1a\n"){"image/png"}
            else if bytes.starts_with(b"GIF87a")||bytes.starts_with(b"GIF89a"){"image/gif"}
            else if bytes.starts_with(b"RIFF")&&bytes.get(8..12)==Some(b"WEBP"){"image/webp"}
            else{return None;};
        Some(Self{bytes,content_type})
    }
}

fn xml_field<'a>(xml:&'a str,name:&str)->Option<&'a str>{
    let value=xml.split_once(&format!("<{name}>"))?.1.split_once(&format!("</{name}>"))?.0.trim();
    Some(value.strip_prefix("<![CDATA[").and_then(|v|v.strip_suffix("]]>")).unwrap_or(value).trim())
}

fn avatar_url(xml:&str,id:&str)->Option<String>{
    if xml_field(xml,"steamID64")?!=id{return None;}
    let url=xml_field(xml,"avatarFull")?;
    // Only Valve avatar CDNs and avatar hashes may be requested. The caller
    // supplies a SteamID, never a URL or a filesystem path.
    let file=["https://avatars.steamstatic.com/","https://avatars.akamai.steamstatic.com/",
        "https://avatars.fastly.steamstatic.com/","https://avatars.cloudflare.steamstatic.com/",
        "https://cdn.akamai.steamstatic.com/steamcommunity/public/images/avatars/",
        "https://steamcdn-a.akamaihd.net/steamcommunity/public/images/avatars/"]
        .iter().find_map(|prefix|url.strip_prefix(*prefix))?;
    let file=if let Some((folder,name))=file.split_once('/'){
        if folder.len()!=2||!folder.bytes().all(|b|b.is_ascii_hexdigit()){return None;}
        name
    }else{file};
    let hash=file.strip_suffix("_full.jpg").or_else(||file.strip_suffix(".jpg"))?;
    if hash.len()!=40||!hash.bytes().all(|b|b.is_ascii_hexdigit()){return None;}
    Some(url.to_owned())
}

pub fn steam_avatar(id:&str,cache:&Path,offline:bool)->Option<Avatar>{
    steam_avatar_with(id,cache,offline,crate::assets::download)
}

fn steam_avatar_with(id:&str,cache:&Path,offline:bool,mut fetch:impl FnMut(&str,usize)->Result<Vec<u8>,String>)->Option<Avatar>{
    if id.len()!=17||steam_id64(id.parse().ok()?).as_deref()!=Some(id){return None;}
    let path=cache.join(format!("steam_avatar_{id}.img"));
    let cached=fs::read(&path).ok().and_then(Avatar::from_bytes);
    let fresh=fs::metadata(&path).and_then(|m|m.modified()).ok()
        .and_then(|at|SystemTime::now().duration_since(at).ok()).is_some_and(|age|age<CACHE_AGE);
    if offline||(cached.is_some()&&fresh){return cached;}
    static ATTEMPTS:OnceLock<Mutex<HashMap<std::path::PathBuf,Instant>>>=OnceLock::new();
    {
        let mut attempts=ATTEMPTS.get_or_init(||Mutex::new(HashMap::new())).lock().ok()?;
        if attempts.get(&path).is_some_and(|at|at.elapsed()<Duration::from_secs(300)){return cached;}
        attempts.insert(path.clone(),Instant::now());
    }
    let downloaded=(||{
        let profile=fetch(&format!("https://steamcommunity.com/profiles/{id}/?xml=1"),1_000_000).ok()?;
        let url=avatar_url(std::str::from_utf8(&profile).ok()?,id)?;
        Avatar::from_bytes(fetch(&url,MAX_IMAGE).ok()?)
    })();
    if let Some(avatar)=downloaded{
        let _=fs::create_dir_all(cache);
        let temporary=path.with_extension("part");
        if fs::write(&temporary,&avatar.bytes).is_ok(){let _=fs::rename(&temporary,&path);}
        Some(avatar)
    }else{cached}
}

#[cfg(test)]mod tests{
    use super::*;
    const ID:&str="76561197960287930";
    const URL:&str="https://avatars.fastly.steamstatic.com/0123456789abcdef0123456789abcdef01234567_full.jpg";
    fn profile(id:&str,url:&str)->String{format!("<profile><steamID64>{id}</steamID64><avatarFull><![CDATA[{url}]]></avatarFull></profile>")}
    fn cache_dir()->std::path::PathBuf{std::env::temp_dir().join(format!("radar-avatars-{}-{}",std::process::id(),rand::random::<u64>()))}

    #[test]fn steam_ids_preserve_precision_and_exclude_bots_and_non_player_accounts(){
        assert_eq!(steam_id64(76561197960287930).as_deref(),Some(ID));
        for id in [0,u64::MAX,76561197960265728,0x0170000100000001,0x0210000100000001]{assert!(steam_id64(id).is_none());}
    }
    #[test]fn profile_must_match_the_player_and_use_a_valve_avatar_url(){
        assert_eq!(avatar_url(&profile(ID,URL),ID).as_deref(),Some(URL));
        for host in ["avatars.steamstatic.com","avatars.akamai.steamstatic.com","avatars.cloudflare.steamstatic.com"]{
            let url=URL.replace("avatars.fastly.steamstatic.com",host);
            assert_eq!(avatar_url(&profile(ID,&url),ID).as_deref(),Some(url.as_str()));
        }
        assert_eq!(avatar_url(&profile(ID,URL).replace("<![CDATA[","").replace("]]>",""),ID).as_deref(),Some(URL));
        assert!(avatar_url(&profile("76561198000000001",URL),ID).is_none());
        for url in ["https://attacker.invalid/avatar.jpg","https://avatars.akamai.steamstatic.com.attacker.invalid/a.jpg",
            "https://avatars.steamstatic.com/../../local.jpg","https://avatars.steamstatic.com/<script>.jpg"]{
            assert!(avatar_url(&profile(ID,url),ID).is_none());
        }
        assert!(avatar_url("<html>Steam unavailable</html>",ID).is_none());
    }
    #[test]fn avatar_download_is_cached_and_available_offline_with_the_right_mime_type(){
        let cache=cache_dir();let png=include_bytes!("../assets/bomb_ground.png");let mut calls=Vec::new();
        let avatar=steam_avatar_with(ID,&cache,false,|url,_|{
            calls.push(url.to_owned());
            Ok(if url==URL{png.to_vec()}else{profile(ID,URL).into_bytes()})
        }).unwrap();
        assert_eq!(calls,vec![format!("https://steamcommunity.com/profiles/{ID}/?xml=1"),URL.into()]);
        assert_eq!(avatar.content_type,"image/png");assert_eq!(avatar.bytes,png);
        for offline in [false,true]{
            let avatar=steam_avatar_with(ID,&cache,offline,|_,_|panic!("cached avatar used the network")).unwrap();
            assert_eq!(avatar.bytes,png);
        }
        let jpeg=vec![0xff,0xd8,0xff,0xe0,0xff,0xd9];
        fs::write(cache.join(format!("steam_avatar_{ID}.img")),&jpeg).unwrap();
        assert_eq!(steam_avatar_with(ID,&cache,true,|_,_|unreachable!()).unwrap().content_type,"image/jpeg");
        fs::remove_dir_all(cache).unwrap();
    }
    #[test]fn failed_downloads_are_throttled_and_never_cached_as_avatars(){
        let cache=cache_dir();let mut calls=0;
        for _ in 0..2{assert!(steam_avatar_with(ID,&cache,false,|_,_|{calls+=1;Err("offline".into())}).is_none());}
        assert_eq!(calls,1);assert!(!cache.exists());
        for id in ["0","../../etc/passwd","+76561197960287930","76561197960265728"]{
            assert!(steam_avatar_with(id,&cache,false,|_,_|panic!("invalid ID used the network")).is_none());
        }
        let cache=cache_dir();
        assert!(steam_avatar_with(ID,&cache,false,|url,_|Ok(if url==URL{b"<html>error</html>".to_vec()}else{profile(ID,URL).into_bytes()})).is_none());
        assert!(!cache.exists());
    }
    #[test]fn expired_avatars_refresh_but_remain_available_when_steam_is_unreachable(){
        let png=include_bytes!("../assets/bomb_ground.png");
        for fails in [false,true]{
            let cache=cache_dir();fs::create_dir_all(&cache).unwrap();
            let path=cache.join(format!("steam_avatar_{ID}.img"));
            let jpeg=vec![0xff,0xd8,0xff,0xe0,0xff,0xd9];fs::write(&path,&jpeg).unwrap();
            fs::File::options().write(true).open(&path).unwrap()
                .set_times(fs::FileTimes::new().set_modified(SystemTime::UNIX_EPOCH)).unwrap();
            let avatar=steam_avatar_with(ID,&cache,false,|url,_|{
                if fails{Err("Steam unavailable".into())}
                else{Ok(if url==URL{png.to_vec()}else{profile(ID,URL).into_bytes()})}
            }).unwrap();
            assert_eq!(avatar.bytes,if fails{jpeg}else{png.to_vec()});
            assert_eq!(fs::read(&path).unwrap(),avatar.bytes);
            fs::remove_dir_all(cache).unwrap();
        }
    }
}
