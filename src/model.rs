use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    pub id: u32,
    pub name: String,
    pub team: u8,
    pub local: bool,
    pub health: Option<i32>,
    pub life_state: String,
    pub health_verified: bool,
    pub armor: i32,
    pub helmet: bool,
    pub defuser: bool,
    pub money: Option<i32>,
    pub position: [f32; 3],
    pub yaw: Option<f32>,
    pub weapon: String,
    pub inventory: Vec<String>,
    pub has_c4: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c4_source: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalMotion {
    pub speed:Option<f32>,pub on_ground:Option<bool>,pub ducking:Option<bool>,pub shots_fired:Option<i32>,pub weapon:Option<String>
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NoiseState {
    pub kind:String,pub radius_units:f32,pub approximate:bool,pub age_ms:u32
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub status: String,
    pub pid: u32,
    pub local_team: u8,
    pub players: Vec<Player>,
    pub bomb_carrier_id: Option<u32>,
    pub bomb_carrier_source: Option<String>,
    pub c4_unowned: bool,
    pub c4_global_owner_id: Option<u32>,
    pub details_read: bool,
    pub diagnostics: Value,
    pub time: f64,
    pub source_hz: f64,
    pub target_hz: u32,
    pub message: Option<String>,
    pub game_map: String,
    pub game_map_source: String,
    #[serde(default,skip_serializing_if="Option::is_none")] pub local_motion:Option<LocalMotion>,
    #[serde(default,skip_serializing_if="Option::is_none")] pub noise:Option<NoiseState>,
}

pub fn now_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

impl Snapshot {
    pub fn status(status: &str, message: &str) -> Self {
        Self {
            status: status.to_owned(), pid: 0, local_team: 0, players: Vec::new(),
            bomb_carrier_id: None, bomb_carrier_source: None, c4_unowned: false,
            c4_global_owner_id: None, details_read: false, diagnostics: json!({}),
            time: now_secs(), source_hz: 0., target_hz: 60,
            message: if message.is_empty() { None } else { Some(message.to_owned()) },
            game_map: String::new(), game_map_source: String::new(),local_motion:None,noise:None,
        }
    }
}

pub fn life_state(hp: Option<i32>, alive: Option<bool>) -> &'static str {
    if matches!(hp, Some(1..=200)) { if alive == Some(false) { "unknown" } else { "alive" } }
    else if hp == Some(0) { if alive == Some(true) { "unknown" } else { "dead" } }
    else if hp.is_none() && alive == Some(false) { "dead" }
    else { "unknown" }
}

pub fn normalized_yaw(angles: Option<[f32; 3]>) -> Option<f32> {
    let [pitch,yaw,roll] = angles?;
    if ![pitch,yaw,roll].iter().all(|v| v.is_finite()) || pitch.abs()>91. || roll.abs()>180. || yaw.abs()>36000. { return None; }
    Some((yaw+180.).rem_euclid(360.)-180.)
}

pub fn normalize_map_name(v: &str) -> Option<String> {
    if v.len()>180 {return None;}
    let lower=v.replace('\\',"/").to_ascii_lowercase();
    let mut name=lower.rsplit('/').next()?.to_owned();
    if name.ends_with(".vpk") || name.ends_with(".bsp") {name.truncate(name.len()-4);}
    let prefix = ["de_","cs_","ar_","dz_","workshop_"];
    if !prefix.iter().any(|p| name.starts_with(*p)) || name.len()<5 || name.len()>69 {return None;}
    if !name.as_bytes().iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b==b'_') {return None;}
    Some(name)
}

pub fn unique_bomb_carrier(players: &[Player]) -> Option<u32> {
    let mut carrier=None;
    for p in players {
        if p.team==2 && p.life_state=="alive" && p.has_c4==Some(true) {
            if carrier.is_some(){return None;}
            carrier=Some(p.id);
        }
    }
    carrier
}

pub fn demo_snapshot(t: f64) -> Snapshot {
    let positions:[(f32,f32,u8,&str,&str);7] = [(-1530.,-770.,2,"MIRAGE | AWP","AWP"),(-1170.,-390.,2,"sora","AK-47"),
        (-1980.,-170.,2,"tabasco","Desert Eagle"),(-900.,440.,3,"conductor","M4A1-S"),
        (-1700.,750.,3,"valor","MP9"),(-420.,-630.,3,"player.exe","AWP"),
        (-1250.,80.,3,"demo-user","AK-47")];
    let mut players=Vec::new();
    for (i,(x,y,team,name,weapon)) in positions.into_iter().enumerate() {
        let a=t*(0.15+i as f64*0.019)+i as f64*2.4;
        let dead=i==2;
        let mut inventory=vec![weapon.to_string(), (if team==2 {"Glock-18"} else {"USP-S"}).into(),"Flashbang".into(),"Smoke".into()];
        if i==1 {inventory.push("C4".into());}
        players.push(Player {id:i as u32+1,name:name.to_owned(),team,local:i==6,
            health:Some(if dead {0} else {100-8*(i as i32%3)}),
            life_state:if dead {"dead"} else {"alive"}.into(),health_verified:!dead,
            armor:if i%2==1 {100} else {50},helmet:i%2==1,defuser:false,money:Some(2500+i as i32*700),
            position:[x+(90.*a.cos()) as f32,y+(65.*a.sin()) as f32,-150.],
            yaw:if dead {None} else {Some(((a+0.4).to_degrees()%360.-180.) as f32)},
            weapon:weapon.into(), inventory, has_c4:Some(i==1),c4_source:None});
    }
    Snapshot { status:"demo".into(),local_team:3,players,
        local_motion:Some(LocalMotion{speed:Some(230.),on_ground:Some(true),ducking:Some(false),shots_fired:Some(0),weapon:Some("AK-47".into())}),
        bomb_carrier_id:Some(2),bomb_carrier_source:Some("inventory".into()),
        ..Snapshot::status("demo","") }
}

#[derive(Clone,Debug,Default)]
pub struct Diagnostics {
    pub counters: HashMap<String,Value>,
}
impl Diagnostics {
    pub fn add(&mut self, key:&str) {let next=self.num(key)+1;self.counters.insert(key.to_owned(),json!(next));}
    pub fn set(&mut self, key:&str, value: Value) {self.counters.insert(key.to_owned(),value);}
    pub fn num(&self, key:&str) -> u64 {self.counters.get(key).and_then(Value::as_u64).unwrap_or(0)}
    pub fn value(self)->Value {json!(self.counters)}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn life_flags(){assert_eq!(life_state(Some(100),Some(true)),"alive");assert_eq!(life_state(Some(0),Some(false)),"dead");assert_eq!(life_state(None,None),"unknown");}
    #[test] fn yaw(){assert_eq!(normalized_yaw(Some([0.,450.,0.])),Some(90.));assert!(normalized_yaw(Some([0.,f32::NAN,0.])).is_none());}
    #[test] fn map(){assert_eq!(normalize_map_name("maps/de_mirage.vpk").unwrap(),"de_mirage");assert!(normalize_map_name("../../bad.cfg").is_none());}
    #[test] fn demo(){let d=demo_snapshot(2.);assert_eq!(d.players.len(),7);assert_eq!(d.bomb_carrier_id,Some(2));}
    #[test] fn json_browser_contract(){
        let v=serde_json::to_value(demo_snapshot(1.)).unwrap();
        assert_eq!(v["status"],"demo");assert!(v["players"][1]["position"].is_array());
        assert_eq!(v["bomb_carrier_id"],2);assert!(v["players"][1]["inventory"].is_array());
        assert!(v["players"][1]["has_c4"].as_bool().unwrap());
    }
}