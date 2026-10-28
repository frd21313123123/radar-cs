use crate::model::normalize_map_name;
use std::{collections::HashSet,fs,path::{Path,PathBuf},process::Command};

pub fn cfg_text(port:u16,secret:&str)->String{
    format!("\"CAS Radar Map Auto\"\n{{\n \"uri\" \"http://127.0.0.1:{port}/gsi\"\n \"timeout\" \"5.0\"\n \"buffer\" \"0.1\"\n \"throttle\" \"0.5\"\n \"heartbeat\" \"1.0\"\n \"auth\" {{ \"token\" \"{secret}\" }}\n \"data\" {{ \"provider\" \"1\" \"map\" \"1\" }}\n}}\n")
}

fn steam_roots()->Vec<PathBuf>{
    let mut roots=Vec::new();
    #[cfg(windows)]{
        if let Ok(result)=Command::new("reg").args(["query",r"HKCU\Software\Valve\Steam","/v","SteamPath"]).output(){
            let text=String::from_utf8_lossy(&result.stdout);
            for line in text.lines(){if let Some(v)=line.split("REG_SZ").nth(1){roots.push(PathBuf::from(v.trim()));}}
        }
        for d in b'C'..=b'Z' {for sub in ["Steam","Program Files (x86)/Steam","Program Files/Steam","SteamLibrary"] {
            let p=PathBuf::from(format!("{}:/{}",d as char,sub));if p.exists(){roots.push(p);}
        }}
    }
    roots
}
pub fn candidate_cfg_dirs()->Vec<PathBuf>{
    let mut directories=Vec::new();let mut dedup=HashSet::new();
    let sub=Path::new("steamapps/common/Counter-Strike Global Offensive/game/csgo/cfg");
    for root in steam_roots(){
        if dedup.insert(root.clone()){directories.push(root.join(sub));}
        if let Ok(vdf)=fs::read_to_string(root.join("steamapps/libraryfolders.vdf")){
            for line in vdf.lines(){if let Some((_,right))=line.split_once("\"path\"") {
                let raw=right.split('"').filter(|x|!x.trim().is_empty()).next().unwrap_or("");
                let path=PathBuf::from(raw.trim().replace("\\\\","\\"));
                if path.is_dir()&&dedup.insert(path.clone()){directories.push(path.join(sub));}
            }}
        }
    }
    directories
}
pub fn install_cfg(port:u16,secret:&str,explicit:Option<&Path>)->Result<(PathBuf,bool),String>{
    let dirs=if let Some(dir)=explicit {vec![dir.to_owned()]}else{candidate_cfg_dirs()};
    for folder in dirs{
        if !folder.is_dir(){continue;}
        let dest=folder.join("gamestate_integration_cas_radar.cfg");let content=cfg_text(port,secret);
        let changed=fs::read_to_string(&dest).map_or(true,|v|v!=content);
        if changed{fs::write(&dest,content).map_err(|e|e.to_string())?;}
        return Ok((dest,changed));
    }
    Err("CS2 game/csgo/cfg folder not found; specify --cs2-cfg".into())
}
pub fn gsi_map(message:&serde_json::Value)->Option<String>{normalize_map_name(message.get("map")?.get("name")?.as_str()?)}
pub fn token_equal(a:&str,b:&str)->bool{let x=a.as_bytes();let y=b.as_bytes();let mut diff=x.len()^y.len();for i in 0..x.len().max(y.len()){diff|=(x.get(i).copied().unwrap_or(0)^y.get(i).copied().unwrap_or(0)) as usize;}diff==0}
#[cfg(test)]mod tests{use super::*;#[test]fn gsi(){assert!(cfg_text(8765,"secret").contains("/gsi"));assert_eq!(gsi_map(&serde_json::json!({"map":{"name":"de_mirage"}})).as_deref(),Some("de_mirage"));}#[test]fn constant_time_compare(){assert!(token_equal("abc","abc"));assert!(!token_equal("ab","abc"));}}