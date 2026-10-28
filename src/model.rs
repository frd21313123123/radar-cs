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
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct BombWorld {
    pub position: [f32; 3],
    pub status: String, // "dropped" or "planted"
    pub source: String, // "entity", "planted_entity", or "recent"
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GrenadeView {
    pub id:u32,
    pub kind:String,
    pub state:String,
    pub position:[f32;3],
    pub radius_units:f32,
    pub trajectory:Vec<[f32;3]>,
    pub fire_cells:Vec<[f32;3]>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObserverView {
    /// Entity-list index of the observed pawn; stable across snapshots.
    pub target_entity_id: u32,
    pub player_id: Option<u32>,
    pub name: String,
    pub position: [f32; 3],
    pub yaw: Option<f32>,
    pub mode: u8,
}

/// Camera focus used only for the small native map.
#[derive(Clone, Debug, PartialEq)]
pub struct RadarFocus {
    pub entity_id: u32,
    pub position: [f32; 3],
    pub yaw: Option<f32>,
    pub name: String,
    pub spectating: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub status: String,
    pub pid: u32,
    pub local_team: u8,
    pub players: Vec<Player>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub observer_view: Option<ObserverView>,
    pub bomb_carrier_id: Option<u32>,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub bomb_world: Option<BombWorld>,
    #[serde(default)] pub grenades:Vec<GrenadeView>,
    #[serde(default)] pub grenades_read:bool,
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
    /// The living local player always takes precedence; when dead, optionally
    /// follow the confirmed observer target (not the corpse position).
    pub fn radar_focus(&self, follow_spectator: bool) -> Option<RadarFocus> {
        if let Some(p)=self.players.iter().find(|p|p.local && p.life_state=="alive") {
            return Some(RadarFocus{entity_id:p.id,position:p.position,
                yaw:p.yaw,name:p.name.clone(),spectating:false});
        }
        if follow_spectator {
            if let Some(target)=self.observer_view.as_ref(){
                if matches!(target.mode,4|5)
                    && target.position.iter().all(|v|v.is_finite())
                    && target.position!=[0.,0.,0.] {
                    return Some(RadarFocus{entity_id:target.target_entity_id,
                        position:target.position,yaw:target.yaw,
                        name:target.name.clone(),spectating:true});
                }
            }
        }
        None
    }
    pub fn status(status: &str, message: &str) -> Self {
        Self {
            status: status.to_owned(), pid: 0, local_team: 0, players: Vec::new(), observer_view:None,
            bomb_carrier_id: None, bomb_world: None, grenades:Vec::new(), grenades_read:false, bomb_carrier_source: None, c4_unowned: false,
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
        grenades_read:true,
        grenades:vec![
          GrenadeView{id:1001,kind:"smoke".into(),state:"active".into(),position:[-1000.,-300.,-150.],radius_units:160.,trajectory:vec![],fire_cells:vec![]},
          GrenadeView{id:1002,kind:"molotov".into(),state:"active".into(),position:[-1800.,120.,-150.],radius_units:135.,trajectory:vec![],fire_cells:vec![[-1810.,100.,-150.],[-1760.,135.,-150.]]},
          GrenadeView{id:1003,kind:"smoke".into(),state:"flying".into(),position:[-1430.,-430.,-110.],radius_units:160.,
             trajectory:vec![[-1680.,-650.,-140.],[-1600.,-560.,-80.],[-1510.,-490.,-95.],[-1430.,-430.,-110.]],fire_cells:vec![]}
        ],
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
    #[test] fn spectator_focus_follows_target(){
        let mut s=demo_snapshot(1.);
        let watched=s.players.iter().find(|p|p.id==2).unwrap().clone();
        s.players.iter_mut().find(|p|p.local).unwrap().life_state="dead".into();
        s.observer_view=Some(ObserverView{target_entity_id:222,player_id:Some(2),
            name:watched.name.clone(),position:watched.position,yaw:watched.yaw,mode:4});
        let focus=s.radar_focus(true).unwrap();
        assert!(focus.spectating);assert_eq!(focus.entity_id,222);
        assert_eq!(focus.position,watched.position);assert_eq!(focus.yaw,watched.yaw);
        assert!(s.radar_focus(false).is_none());
    }
    #[test] fn spectator_target_switch_and_freecam(){
        let mut s=demo_snapshot(1.);
        s.players.iter_mut().find(|p|p.local).unwrap().life_state="dead".into();
        s.observer_view=Some(ObserverView{target_entity_id:101,player_id:Some(1),
            name:"A".into(),position:[120.,30.,10.],yaw:Some(45.),mode:4});
        assert_eq!(s.radar_focus(true).unwrap().entity_id,101);
        s.observer_view.as_mut().unwrap().target_entity_id=102;
        s.observer_view.as_mut().unwrap().position=[140.,40.,10.];
        assert_eq!(s.radar_focus(true).unwrap().position,[140.,40.,10.]);
        s.observer_view.as_mut().unwrap().mode=6;
        assert!(s.radar_focus(true).is_none());
    }
    #[test] fn living_local_player_takes_priority(){
        let mut s=demo_snapshot(1.);
        s.observer_view=Some(ObserverView{target_entity_id:333,player_id:None,
            name:"Other".into(),position:[50.,50.,50.],yaw:Some(90.),mode:4});
        let f=s.radar_focus(true).unwrap();
        assert!(!f.spectating);assert_eq!(f.name,"demo-user");
    }
    #[test] fn json_browser_contract(){
        let v=serde_json::to_value(demo_snapshot(1.)).unwrap();
        assert_eq!(v["status"],"demo");assert!(v["players"][1]["position"].is_array());
        assert_eq!(v["bomb_carrier_id"],2);assert!(v["players"][1]["inventory"].is_array());
        assert!(v["players"][1]["has_c4"].as_bool().unwrap());
    }
}