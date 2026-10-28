//! Local vector equipment icons and portraits matched to the actual pawn model.
//! Agent metadata: ByMykel/CSGO-API (MIT); character artwork belongs to Valve.
use std::{path::Path,collections::HashMap,sync::{Mutex,OnceLock},time::{Duration,Instant}};
use serde::Deserialize;

pub fn weapon_icon(name:&str)->String {
    let (label,shape)=match name {
        "AK-47"=>("AK", "M4 13h9l5-5h15l3 3h22v4H35l-3 5-2 8h-7l2-10h-9l-8 6H3z"),
        "AWP"|"SSG 08"|"SCAR-20"|"G3SG1"=>(match name {"AWP"=>"AWP","SSG 08"=>"SSG",_=>"AUTO"},"M3 16h12l5-5h16v3h25v3H34l-3 7h-5l1-7h-9l-8 7H3z M24 6h15v4H24z M28 10h3v5h-3z"),
        "M4A1-S"|"M4A4"|"Galil AR"|"FAMAS"|"AUG"|"SG 553"=>(match name {"M4A1-S"=>"M4-S","M4A4"=>"M4","Galil AR"=>"GALIL","FAMAS"=>"FAMAS","AUG"=>"AUG",_=>"SG"},"M4 12h12l4-4h16v4h24v4H34l-3 10h-6l1-9h-8l-9 6H4z M22 5h12v3H22z"),
        "MAC-10"|"MP9"|"MP7"|"MP5-SD"|"UMP-45"|"P90"|"PP-Bizon"=>(match name {"MAC-10"=>"MAC","MP9"=>"MP9","MP7"=>"MP7","MP5-SD"=>"MP5","UMP-45"=>"UMP","P90"=>"P90",_=>"BIZON"},"M9 11h32v4h15v3H35l-2 8h-6l1-8h-8l-7 5H8z M18 7h18v4H18z"),
        "Nova"|"XM1014"|"Sawed-Off"|"MAG-7"=>(match name {"Nova"=>"NOVA","XM1014"=>"XM","MAG-7"=>"MAG",_=>"SAWED"},"M3 16h14l4-5h39v4H34v4H22l-9 7H3z"),
        "M249"|"Negev"=>(if name=="M249"{"M249"}else{"NEGEV"},"M3 12h13l3-4h20v5h22v4H36v10H23v-9h-7l-9 6H3z"),
        "Knife"=>("KNIFE","M4 22l13-8 5 4-11 9z M20 13L57 4 34 23 24 21z"),
        "C4"=>("C4","M17 8h29v21H17z M22 3h4v6h-4z M30 3h4v6h-4z M23 12h17v5H23z M23 21h3v3h-3z M30 21h3v3h-3z M37 21h3v3h-3z"),
        "Smoke"|"Flashbang"|"HE Grenade"|"Decoy"=>(match name {"Smoke"=>"SMOKE","Flashbang"=>"FLASH","HE Grenade"=>"HE",_=>"DECOY"},"M25 10h14l4 7v10H21V17z M27 4h9v5h-9z M37 5h9v3h-9z"),
        "Molotov"|"Incendiary"=>(if name=="Molotov"{"MOLO"}else{"FIRE"},"M28 4h8v8l7 9v8H21v-8l7-9z M34 3l6-3 5 4-7 2z"),
        "Armor"=>("ARMOR","M14 7l10-4v5h16V3l10 4-5 8v15H19V15z"),
        "Helmet"=>("HELM","M14 19C14-3 50-3 50 19v7H37v-8H14z"),
        "Defuse Kit"=>("KIT","M21 5h22v24H21z M27 1h10v5H27z M25 12h14v3H25z M30 8h4v12h-4z"),
        "Desert Eagle"|"Dual Berettas"|"Five-SeveN"|"Glock-18"|"Tec-9"|"P2000"|"P250"|"USP-S"|"CZ75-Auto"|"R8 Revolver"|"Zeus x27"=>(match name {"Desert Eagle"=>"DEAGLE","Dual Berettas"=>"DUAL","Five-SeveN"=>"5-7","Glock-18"=>"GLOCK","Tec-9"=>"TEC9","P2000"=>"P2000","P250"=>"P250","USP-S"=>"USP","CZ75-Auto"=>"CZ","R8 Revolver"=>"R8",_=>"ZEUS"},"M12 8h39v8H32l-7 14H14l6-16h-8z"),
        _=>("?","M22 10h20v18H22z"),
    };
    format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 44" role="img"><title>{label}</title><path fill="#e9f2f8" fill-rule="evenodd" d="{shape}"/><text x="32" y="41" text-anchor="middle" fill="#e9f2f8" font-family="Arial,sans-serif" font-size="9" font-weight="700">{label}</text></svg>"##)
}

pub const NEUTRAL_AGENT:&str=r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" role="img"><title>Нет изображения агента</title><rect width="64" height="64" rx="10" fill="#203144"/><circle cx="32" cy="23" r="12" fill="#a8bac9"/><path d="M10 61v-9c0-22 44-22 44 0v9" fill="#829aaf"/></svg>"##;

#[derive(Deserialize)]struct Agent {model:String,image:String}
fn agent(model:&str)->Option<&'static Agent> {
    static AGENTS:OnceLock<Vec<Agent>>=OnceLock::new();
    let normalized=model.replace('\\',"/").to_ascii_lowercase();
    let name=normalized.rsplit('/').next()?.trim_end_matches("_c");
    if name.is_empty() || !name.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'.'){return None;}
    AGENTS.get_or_init(||serde_json::from_str(include_str!("../assets/agents.json")).expect("bundled agent metadata"))
        .iter().find(|a|a.model.rsplit('/').next()==Some(name))
}
/// Only bundled, known Valve URLs may be fetched, never a client-provided URL.
pub fn agent_icon(model:&str,cache:&Path,offline:bool)->Option<Vec<u8>> {
    let a=agent(model)?;
    if !a.image.starts_with("https://community.akamai.steamstatic.com/economy/image/"){return None;}
    let name=a.model.rsplit('/').next()?;
    let path=cache.join(format!("agent_{name}.png"));
    let png=|b:&Vec<u8>| b.len()<=2_000_000 && b.starts_with(b"\x89PNG\r\n\x1a\n");
    if let Ok(b)=std::fs::read(&path){if png(&b){return Some(b);}}
    if offline{return None;}
    static ATTEMPTS:OnceLock<Mutex<HashMap<String,Instant>>>=OnceLock::new();
    {
        let mut attempts=ATTEMPTS.get_or_init(||Mutex::new(HashMap::new())).lock().ok()?;
        if attempts.get(name).is_some_and(|at|at.elapsed()<Duration::from_secs(300)){return None;}
        attempts.insert(name.into(),Instant::now());
    }
    let bytes=crate::assets::download(&a.image,2_000_000).ok().filter(png)?;
    let _=std::fs::create_dir_all(cache);
    let _=std::fs::write(path,&bytes);
    Some(bytes)
}

#[cfg(test)]mod tests {
 use super::*;
 #[test]fn model_path_matches_known_skin_only(){
  assert!(agent("characters/models/tm_professional/tm_professional_varf5.vmdl_c").is_some());
  assert!(agent("https://attacker.invalid/evil").is_none());
  assert!(agent("../../unknown.vmdl").is_none());
  assert!(agent("").is_none());
 }
 #[test]fn equipment_is_bundled_and_untrusted_names_are_not_markup(){
  for name in ["AK-47","AWP","Smoke","Flashbang","C4","Armor","Helmet","Defuse Kit"] {
   let svg=weapon_icon(name);assert!(svg.starts_with("<svg"));assert!(!svg.contains("<title>?</title>"));
  }
  assert!(!weapon_icon("<script>alert(1)</script>").contains("script"));
  assert_ne!(weapon_icon("AWP"),weapon_icon("AK-47"));
 }
}
