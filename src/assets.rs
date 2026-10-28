use serde_json::{Value,json};
use std::{fs,io::Read,path::{Path,PathBuf},time::{Duration,SystemTime}};
use crate::model::normalize_map_name;

pub const OFFSETS_SRC:&str="https://raw.githubusercontent.com/a2x/cs2-dumper/main/output/";
pub const MAP_SRC:&str="https://raw.githubusercontent.com/MurkyYT/cs2-map-icons/main/data/available.json";
pub const RADARS:&str="https://raw.githubusercontent.com/MurkyYT/cs2-map-icons/main/images/radars/";
const UA:&str="CAS-External-Radar-Rust/0.9";

pub fn download(url:&str,limit:usize)->Result<Vec<u8>,String>{
    let response=ureq::get(url).set("User-Agent",UA).timeout(Duration::from_secs(8)).call().map_err(|e|e.to_string())?;
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
    json!({"maps":maps})
}
pub fn load_maps(cache:&Path,offline:bool,force:bool)->Value{
    let got=cached_json(MAP_SRC,&cache.join("maps.json"),offline,force);
    if got.as_ref().and_then(|v|v["maps"].as_object()).is_some(){got.unwrap()}else{fallback_maps()}
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
    let bytes=if path.is_file(){fs::read(path).map_err(|e|e.to_string())?}
       else{if offline{return Err("Image unavailable offline".into());}let b=download(url,8_000_000)?;if b.starts_with(b"\x89PNG\r\n\x1a\n"){let _=fs::write(&path,&b);}b};
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n"){return Err("Invalid PNG".into());}
    Ok(bytes)
}

#[cfg(test)]mod tests{use super::*;#[test]fn maps_valid(){let m=fallback_maps();assert!(m["maps"]["de_mirage"]["radar_info"]["scale"].is_number());}#[test]fn reject_path(){assert!(map_image("../bad","default",&fallback_maps(),Path::new("cache"),true).is_err());}}