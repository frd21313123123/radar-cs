use serde_json::{Value,json};
use std::{fs,io::Read,path::{Path,PathBuf},time::{Duration,SystemTime}};
use crate::model::normalize_map_name;

pub const OFFSETS_SRC:&str="https://raw.githubusercontent.com/a2x/cs2-dumper/main/output/";
pub const MAP_SRC:&str="https://raw.githubusercontent.com/MurkyYT/cs2-map-icons/main/data/available.json";
pub const RADARS:&str="https://raw.githubusercontent.com/MurkyYT/cs2-map-icons/main/images/radars/";
const UA:&str="CAS-External-Radar-Rust/0.9";

fn download_agent()->Result<ureq::Agent,String>{
    use ureq::rustls::{self,pki_types::{CertificateDer,pem::PemObject}};
    // Respect the user's configured route and trust store. ureq does not
    // consult proxy environment variables by default. TLS stays verified.
    let mut builder=ureq::AgentBuilder::new().try_proxy_from_env(true);
    if let Some(path)=std::env::var_os("SSL_CERT_FILE").or_else(||std::env::var_os("REQUESTS_CA_BUNDLE")){
        let certs=CertificateDer::pem_file_iter(std::path::PathBuf::from(path))
            .map_err(|_|"Configured CA bundle cannot be read".to_string())?;
        let mut roots=rustls::RootCertStore::empty();
        for cert in certs{
            let cert=cert.map_err(|_|"Configured CA bundle has invalid PEM".to_string())?;
            roots.add(cert).map_err(|_|"Configured CA bundle has an invalid certificate".to_string())?;
        }
        if roots.is_empty(){return Err("Configured CA bundle has no certificates".into());}
        let tls=rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
        builder=builder.tls_config(std::sync::Arc::new(tls));
    }
    Ok(builder.build())
}
pub fn download(url:&str,limit:usize)->Result<Vec<u8>,String>{
    static AGENT:std::sync::OnceLock<Result<ureq::Agent,String>>=std::sync::OnceLock::new();
    let agent=AGENT.get_or_init(download_agent).as_ref().map_err(Clone::clone)?;
    let response=agent.get(url).set("User-Agent",UA).timeout(Duration::from_secs(8)).call().map_err(|e|e.to_string())?;
    let mut buffer=Vec::new();
    response.into_reader().take(limit as u64+1).read_to_end(&mut buffer).map_err(|e|e.to_string())?;
    if buffer.len()>limit {Err(format!("remote asset too large: {url}"))}else{Ok(buffer)}
}
pub fn cached_json(url:&str,path:&Path,offline:bool,force:bool)->Option<Value>{
    let stale=fs::metadata(path).and_then(|m|m.modified()).ok().and_then(|t|SystemTime::now().duration_since(t).ok()).map_or(true,|d|d.as_secs()>86400);
    if !offline && (force||stale){
        match download(url,2_000_000).and_then(|b|serde_json::from_slice::<Value>(&b).map(|json|(json,b)).map_err(|e|e.to_string())){
            Ok((obj,bytes))=>{
                if let Some(dir)=path.parent(){let _=fs::create_dir_all(dir);}
                let tmp=path.with_extension("part");
                if fs::write(&tmp,&bytes).is_ok(){let _=fs::rename(tmp,path);}return Some(obj);
            }
            Err(e)=>eprintln!("[cache] {url}: {e}; using local copy"),
        }
    }
    fs::read(path).ok().and_then(|b|serde_json::from_slice::<Value>(&b).ok())
}
/// Normalized positions taken from CS2 overview metadata (October 2026).
/// Coordinates are fractions of the 1024x1024 radar image, not world units.
/// Unsupported maps intentionally have no labels instead of fabricated sites.
pub fn bombsites_for_map(name:&str)->Option<[[f32;2];2]> {
    Some(match name {
        "de_mirage"=>[[0.54,0.76],[0.23,0.28]],
        "de_dust2"=>[[0.80,0.16],[0.21,0.12]],
        "de_inferno"|"de_inferno_s2"=>[[0.81,0.69],[0.49,0.22]],
        "de_ancient"|"de_ancient_night"|"de_ancient_v1"|"de_ancient_v2"=>[[0.31,0.25],[0.80,0.40]],
        // Anubis is visually anchored to Valve's radar; its overview metadata
        // does not currently publish explicit bombsite coordinates.
        "de_anubis"=>[[0.73,0.26],[0.32,0.48]],
        "de_nuke"=>[[0.58,0.48],[0.58,0.58]],
        "de_train"=>[[0.63,0.49],[0.52,0.76]],
        "de_vertigo"=>[[0.705,0.585],[0.222,0.223]],
        "de_overpass"|"de_overpass_2v2"=>[[0.55,0.23],[0.70,0.31]],
        "de_cache"=>[[0.325,0.26],[0.345,0.79]],
        _=>return None,
    })
}
fn with_bombsites(mut data:Value)->Value {
    if let Some(maps)=data.get_mut("maps").and_then(Value::as_object_mut) {
        for (name,value) in maps.iter_mut(){
            if let (Some(points),Some(object))=(bombsites_for_map(name),value.as_object_mut()){
                object.insert("bombsites".into(),json!({
                    "A":{"x":points[0][0],"y":points[0][1]},
                    "B":{"x":points[1][0],"y":points[1][1]}
                }));
            }
        }
    }
    data
}
pub fn fallback_maps()->Value{
    let defs:[(&str,i32,i32,f64,&str,Option<i32>);8]=[
        ("de_mirage",-3230,1713,5.,"Mirage",None),("de_dust2",-2476,3239,4.4,"Dust II",None),
        ("de_inferno",-2087,3870,4.9,"Inferno",None),("de_ancient",-2953,2164,5.,"Ancient",None),
        ("de_anubis",-2796,3328,5.22,"Anubis",None),("de_nuke",-3453,2887,7.,"Nuke",Some(-495)),
        ("de_train",-2308,2078,4.082077,"Train",Some(-50)),("de_vertigo",-3168,1762,4.,"Vertigo",Some(11700))];
    let mut maps=serde_json::Map::new();
    for (key,x,y,scale,name,alt) in defs{
        let layers=if let Some(a)=alt {json!({"default":{"AltitudeMin":a,"AltitudeMax":20000},"lower":{"AltitudeMin":-10000,"AltitudeMax":a}})}else{json!({})};
        let mut paths=vec![format!("{RADARS}{key}_radar_psd.png")];
        if alt.is_some(){paths.insert(0,format!("{RADARS}{key}_lower_radar_psd.png"));}
        maps.insert(key.into(),json!({"display_name":name,"radar_paths":paths,
          "radar_info":{"pos_x":x,"pos_y":y,"scale":scale,"verticalsections":layers}}));
    }
    with_bombsites(json!({"maps":maps}))
}
const MAP_REFRESH_MARKER:&str="map-images.refresh";
fn request_map_image_refresh(cache:&Path,offline:bool,force:bool){
    if force&&!offline {
        // Mark existing PNGs stale without downloading every map at startup.
        // Offline mode retains working images even when a refresh was requested.
        if fs::create_dir_all(cache).and_then(|_|fs::write(cache.join(MAP_REFRESH_MARKER),b"")).is_err(){
            eprintln!("[cache] Cannot record map image refresh request");
        }
    }
}
fn map_cache_needs_refresh(path:&Path,cache:&Path,now:SystemTime)->bool{
    let modified=match fs::metadata(path).and_then(|m|m.modified()){Ok(at)=>at,Err(_)=>return true};
    let expired=now.duration_since(modified).is_ok_and(|age|age>=Duration::from_secs(86400));
    let requested=fs::metadata(cache.join(MAP_REFRESH_MARKER)).and_then(|m|m.modified()).ok()
        .is_some_and(|marker|modified<marker);
    expired||requested
}

/// Refresh JSON metadata now; PNGs older than 24h or an online --refresh marker
/// refresh only when requested. A failed fetch retains an existing valid image.
pub fn load_maps(cache:&Path,offline:bool,force:bool)->Value{
    request_map_image_refresh(cache,offline,force);
    let got=cached_json(MAP_SRC,&cache.join("maps.json"),offline,force);
    if got.as_ref().and_then(|v|v["maps"].as_object()).is_some(){with_bombsites(got.unwrap())}else{fallback_maps()}
}
/// Pick the same floor image and world projection metadata at a height boundary.
/// Unknown height keeps the overview default; never invent another floor.
pub fn map_layer_for_height(radar_info:&Value,height:Option<f32>)->String{
    let Some(height)=height.filter(|v|v.is_finite()) else{return "default".into();};
    let Some(layers)=radar_info.get("verticalsections").and_then(Value::as_object) else{return "default".into();};
    for (name,layer) in layers {
        let (Some(min),Some(max))=(layer["AltitudeMin"].as_f64(),layer["AltitudeMax"].as_f64()) else{continue;};
        if min.is_finite()&&max.is_finite()&&min<max&&(height as f64)>=min&&(height as f64)<max
            && name.len()<=20 && name.bytes().all(|b|b.is_ascii_lowercase()||b==b'_') {
            return name.clone();
        }
    }
    "default".into()
}

// A signature alone accepts a truncated cache file that Windows cannot decode.
// Check bounded chunk framing and the image header before persisting/reusing PNGs.
fn valid_map_png(bytes:&[u8])->bool{
    if bytes.len()>8_000_000 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n"){return false;}
    let mut at=8usize;let mut header=false;let mut pixels=false;
    while let Some(chunk)=bytes.get(at..at+8){
        let len=u32::from_be_bytes(chunk[..4].try_into().unwrap()) as usize;
        let Some(end)=at.checked_add(12).and_then(|n|n.checked_add(len)) else{return false;};
        if end>bytes.len(){return false;}
        let kind=&chunk[4..8];
        if !header {
            if kind!=b"IHDR"||len!=13{return false;}
            let w=u32::from_be_bytes(bytes[at+8..at+12].try_into().unwrap());
            let h=u32::from_be_bytes(bytes[at+12..at+16].try_into().unwrap());
            if !(1..=8192).contains(&w)||!(1..=8192).contains(&h){return false;}
            header=true;
        } else if kind==b"IHDR" {return false;}
        if kind==b"IDAT"{pixels=true;}
        if kind==b"IEND"{return len==0&&header&&pixels&&end==bytes.len();}
        at=end;
    }
    false
}

fn refreshed_map_image(cached:Option<Vec<u8>>,downloaded:Result<Vec<u8>,String>)->Result<(Vec<u8>,bool),String>{
    let downloaded=downloaded.and_then(|bytes|if valid_map_png(&bytes){Ok(bytes)}else{
        Err("Downloaded map image is invalid or incomplete".into())
    });
    match downloaded {
        Ok(bytes)=>Ok((bytes,true)),
        Err(error)=>match cached.filter(|bytes|valid_map_png(bytes)){
            Some(bytes)=>{eprintln!("[cache] Map refresh failed: {error}; using valid cached image");Ok((bytes,false))},
            None=>Err(error),
        }
    }
}

pub fn map_image(map:&str,layer:&str,maps:&Value,cache:&Path,offline:bool)->Result<Vec<u8>,String>{
    let name=normalize_map_name(map).ok_or("Invalid map name")?;
    if name!=map{return Err("Unexpected map name".into());}
    if layer!="default" && (layer.len()>20||!layer.bytes().all(|b|b.is_ascii_lowercase()||b==b'_')){return Err("invalid layer".into());}
    let info=maps["maps"].get(map).ok_or("Unknown map")?;
    let images=info["radar_paths"].as_array().ok_or("No images listed")?;
    let prefix=if layer=="default"{format!("{map}_radar_")}else{format!("{map}_{layer}_radar_")};
    let mut candidates=images.iter().filter_map(Value::as_str).filter(|url|url.starts_with(RADARS)&&url.ends_with(".png"))
        .filter_map(|url|url.rsplit('/').next().and_then(|file|if file.starts_with(&prefix)&&file.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'.'){Some((file,url))}else{None})).collect::<Vec<_>>();
    candidates.sort_by_key(|(file,_)|file.len());
    let (file,url)=candidates.first().ok_or("No map image for layer")?;
    let path:PathBuf=cache.join(file);
    let cached=fs::read(&path).ok();
    let damaged=cached.as_ref().is_some_and(|bytes|!valid_map_png(bytes));
    let cached=cached.filter(|bytes|valid_map_png(bytes));
    if let Some(bytes)=cached.as_ref(){
        if offline||!map_cache_needs_refresh(&path,cache,SystemTime::now()){return Ok(bytes.clone());}
    }
    if offline{return Err(if damaged{"Cached map image is damaged; connect once to refresh it"}else{"Image unavailable offline"}.into());}
    let (bytes,fresh)=refreshed_map_image(cached,download(url,8_000_000))?;
    if !fresh{return Ok(bytes);}
    fs::create_dir_all(cache).map_err(|e|format!("Cannot create map cache: {e}"))?;
    // Decode callers receive verified complete bytes even if a cache replacement
    // fails; do not replace a previously good file with a partial download.
    static NEXT_WRITE:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
    let serial=NEXT_WRITE.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
    let temporary=path.with_extension(format!("{}.{serial}.part",std::process::id()));
    if fs::write(&temporary,&bytes).is_ok(){
        if fs::rename(&temporary,&path).is_err(){
            // A locked or otherwise unreplaceable cache entry may block rename.
            // Remove only an entry still proven invalid, then retry once.
            if fs::read(&path).is_ok_and(|old|!valid_map_png(&old)){
                let _=fs::remove_file(&path);let _=fs::rename(&temporary,&path);
            }
            let _=fs::remove_file(&temporary);
        }
    }
    Ok(bytes)
}

#[cfg(test)]mod tests{
    use super::*;
    fn set_modified(path:&Path,time:SystemTime){
        fs::File::options().write(true).open(path).unwrap()
            .set_times(fs::FileTimes::new().set_modified(time)).unwrap();
    }
    #[test]fn refresh_marker_and_24h_expiry_apply_only_to_older_images(){
        let cache=std::env::temp_dir().join(format!("cas-map-refresh-test-{}",std::process::id()));
        fs::create_dir_all(&cache).unwrap();let path=cache.join("de_nuke_radar_psd.png");
        fs::write(&path,include_bytes!("../assets/bomb_ground.png")).unwrap();
        let now=SystemTime::now();let old=now-Duration::from_secs(86401);
        set_modified(&path,now-Duration::from_secs(60));
        assert!(!map_cache_needs_refresh(&path,&cache,now));
        request_map_image_refresh(&cache,false,true);
        let marker=cache.join(MAP_REFRESH_MARKER);set_modified(&marker,now);
        assert!(map_cache_needs_refresh(&path,&cache,now));
        set_modified(&path,now+Duration::from_secs(1));
        assert!(!map_cache_needs_refresh(&path,&cache,now));
        fs::remove_file(&marker).unwrap();set_modified(&path,old);
        assert!(map_cache_needs_refresh(&path,&cache,now));
        // Expiry/refresh never prevents offline use of a structurally valid PNG.
        assert!(map_image("de_nuke","default",&fallback_maps(),&cache,true).is_ok());
        fs::remove_dir_all(cache).unwrap();
    }
    #[test]fn offline_force_does_not_request_image_refresh(){
        let cache=std::env::temp_dir().join(format!("cas-offline-refresh-test-{}",std::process::id()));
        fs::create_dir_all(&cache).unwrap();request_map_image_refresh(&cache,true,true);
        assert!(!cache.join(MAP_REFRESH_MARKER).exists());
        request_map_image_refresh(&cache,false,false);assert!(!cache.join(MAP_REFRESH_MARKER).exists());
        request_map_image_refresh(&cache,false,true);assert!(cache.join(MAP_REFRESH_MARKER).is_file());
        fs::remove_dir_all(cache).unwrap();
    }
    #[test]fn failed_refresh_keeps_valid_cache_without_accepting_invalid_downloads(){
        let valid=include_bytes!("../assets/bomb_ground.png").to_vec();
        let (bytes,fresh)=refreshed_map_image(Some(valid.clone()),Err("network unavailable".into())).unwrap();
        assert_eq!(bytes,valid);assert!(!fresh);
        let (bytes,fresh)=refreshed_map_image(Some(valid.clone()),Ok(b"truncated".to_vec())).unwrap();
        assert_eq!(bytes,valid);assert!(!fresh);
        let (bytes,fresh)=refreshed_map_image(None,Ok(valid.clone())).unwrap();
        assert_eq!(bytes,valid);assert!(fresh);
        assert!(refreshed_map_image(Some(b"broken".to_vec()),Err("network unavailable".into())).is_err());
        assert!(refreshed_map_image(None,Ok(b"invalid".to_vec())).is_err());
    }
    #[test]fn nuke_floor_boundary_uses_default_at_minus_495(){
        let maps=fallback_maps();let overview=&maps["maps"]["de_nuke"]["radar_info"];
        assert_eq!(map_layer_for_height(overview,Some(-600.)),"lower");
        assert_eq!(map_layer_for_height(overview,Some(-495.)),"default");
        assert_eq!(map_layer_for_height(overview,Some(0.)),"default");
        assert_eq!(map_layer_for_height(overview,None),"default");
        assert_eq!(map_layer_for_height(overview,Some(f32::NAN)),"default");
    }
    #[test]fn truncated_png_is_not_a_usable_cached_map(){
        let image=include_bytes!("../assets/bomb_ground.png");
        assert!(valid_map_png(image));assert!(!valid_map_png(&image[..image.len()-5]));
        assert!(!valid_map_png(b"\x89PNG\r\n\x1a\n"));
        let path=std::env::temp_dir().join(format!("cas-map-cache-test-{}",std::process::id()));
        fs::create_dir_all(&path).unwrap();
        let cached=path.join("de_nuke_radar_psd.png");fs::write(&cached,b"\x89PNG\r\n\x1a\n").unwrap();
        assert!(map_image("de_nuke","default",&fallback_maps(),&path,true).unwrap_err().contains("damaged"));
        fs::write(&cached,image).unwrap();
        assert_eq!(map_image("de_nuke","default",&fallback_maps(),&path,true).unwrap(),image);
        fs::remove_dir_all(path).unwrap();
    }
    #[test]fn cached_nuke_floor_paths_are_distinct(){
        let path=std::env::temp_dir().join(format!("cas-nuke-layer-test-{}",std::process::id()));
        fs::create_dir_all(&path).unwrap();let image=include_bytes!("../assets/bomb_ground.png");
        fs::write(path.join("de_nuke_lower_radar_psd.png"),image).unwrap();
        assert!(map_image("de_nuke","default",&fallback_maps(),&path,true).is_err());
        assert_eq!(map_image("de_nuke","lower",&fallback_maps(),&path,true).unwrap(),image);
        fs::remove_dir_all(path).unwrap();
    }
    #[test]fn maps_valid(){let m=fallback_maps();assert!(m["maps"]["de_mirage"]["radar_info"]["scale"].is_number());}
    #[test]fn reject_path(){assert!(map_image("../bad","default",&fallback_maps(),Path::new("cache"),true).is_err());}
    #[test]fn known_sites(){assert_eq!(bombsites_for_map("de_mirage"),Some([[0.54,0.76],[0.23,0.28]]));assert_eq!(bombsites_for_map("de_vertigo"),Some([[0.705,0.585],[0.222,0.223]]));}
    #[test]fn no_sites_on_hostage_maps(){assert!(bombsites_for_map("cs_office").is_none());assert!(bombsites_for_map("de_unknown").is_none());}
    #[test]fn injected_into_offline_and_online_metadata(){
        let offline=fallback_maps();
        assert!((offline["maps"]["de_mirage"]["bombsites"]["A"]["x"].as_f64().unwrap()-0.54).abs()<0.00001);
        let online=with_bombsites(json!({"maps":{"de_cache":{"display_name":"Cache"},"cs_office":{"display_name":"Office"}}}));
        assert!((online["maps"]["de_cache"]["bombsites"]["B"]["y"].as_f64().unwrap()-0.79).abs()<0.00001);
        assert!(online["maps"]["cs_office"].get("bombsites").is_none());
    }
    #[test]fn sites_fit_overview_image(){for name in ["de_mirage","de_dust2","de_inferno","de_ancient","de_anubis","de_nuke","de_train","de_vertigo","de_overpass","de_cache"]{
        for p in bombsites_for_map(name).unwrap(){assert!(p.iter().all(|v|v.is_finite()&&(0.0..=1.0).contains(v)));}
    }}
}