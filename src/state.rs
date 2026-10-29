use crate::model::{Snapshot,normalize_map_name};
use std::time::{Duration,Instant};
use serde_json::{json,Value};

pub struct SharedState {
    pub snap:Snapshot,pub maps:Value,
    map:String,map_source:String,map_at:Instant,manual_map:Option<String>,
}
impl SharedState {
    pub fn new(maps:Value)->Self{Self{snap:Snapshot::status("starting",""),maps,map:String::new(),map_source:String::new(),map_at:Instant::now()-Duration::from_secs(100),manual_map:None}}
    pub fn set_map(&mut self,value:&str,source:&str)->bool{
        let new=match normalize_map_name(value){Some(n)=>n,None=>return false};
        if !["memory","gsi","demo"].contains(&source){return false;}
        if source=="memory"&&self.map_source=="gsi"&&self.map_at.elapsed()<Duration::from_secs(8){return false;}
        self.map=new;self.map_source=source.into();self.map_at=Instant::now();true
    }
    pub fn set_manual_map(&mut self,value:Option<&str>)->Result<(),String>{
        let map=match value{
            Some(name)=>{
                let name=normalize_map_name(name).ok_or("Некорректное имя карты")?;
                if self.maps["maps"].get(&name).is_none(){return Err("Карта отсутствует в списке".into());}
                Some(name)
            },None=>None,
        };
        self.manual_map=map;Ok(())
    }
    /// Missing heartbeats don't change the current level. A confirmed new
    /// level or an actual process disconnect clears/replaces automatic state.
    pub fn clear_automatic_map(&mut self){self.map.clear();self.map_source.clear();}
    pub fn current_map(&self)->&str{self.manual_map.as_deref().unwrap_or(&self.map)}
    pub fn snapshot(&self)->Snapshot{
        let mut s=self.snap.clone();
        s.game_map=self.current_map().into();
        s.game_map_source=if self.manual_map.is_some(){"manual".into()}else{self.map_source.clone()};
        if let Some(d)=s.diagnostics.as_object_mut(){
            d.insert("map_known".into(),json!(!s.game_map.is_empty()));
            d.insert("map_source".into(),json!(s.game_map_source));
        }
        s
    }
}

#[cfg(test)]mod tests{
    use super::*;use crate::assets::fallback_maps;
    #[test]fn quiet_gsi_map_does_not_expire_into_another_map(){
        let mut s=SharedState::new(fallback_maps());assert!(s.set_map("de_nuke","gsi"));
        s.map_at=Instant::now()-Duration::from_secs(60);
        assert_eq!(s.current_map(),"de_nuke");assert_eq!(s.snapshot().game_map,"de_nuke");
        s.clear_automatic_map();assert_eq!(s.current_map(),"");assert_eq!(s.snapshot().game_map,"");
    }
    #[test]fn manual_selection_is_shared_and_auto_resumes_latest_game_map(){
        let mut s=SharedState::new(fallback_maps());s.set_map("de_mirage","gsi");
        s.set_manual_map(Some("de_nuke")).unwrap();assert_eq!(s.snapshot().game_map_source,"manual");
        s.set_map("de_dust2","gsi");assert_eq!(s.current_map(),"de_nuke");
        s.set_manual_map(None).unwrap();assert_eq!(s.current_map(),"de_dust2");
        assert_eq!(s.snapshot().game_map_source,"gsi");
        assert!(s.set_manual_map(Some("../bad")).is_err());assert!(s.set_manual_map(Some("de_nonexistent")).is_err());
        assert_eq!(s.current_map(),"de_dust2");
    }
}
