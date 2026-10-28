use crate::model::{Snapshot,normalize_map_name};
use std::time::{Duration,Instant};
use serde_json::Value;

pub struct SharedState {
    pub snap:Snapshot,pub maps:Value,
    map:String,map_source:String,map_at:Instant,
}
impl SharedState {
    pub fn new(maps:Value)->Self{Self{snap:Snapshot::status("starting",""),maps,map:String::new(),map_source:String::new(),map_at:Instant::now()-Duration::from_secs(100)}}
    pub fn set_map(&mut self,value:&str,source:&str)->bool{
        let new=match normalize_map_name(value){Some(n)=>n,None=>return false};
        if !["memory","gsi","demo"].contains(&source){return false;}
        if source=="memory"&&self.map_source=="gsi"&&self.map_at.elapsed()<Duration::from_secs(8){return false;}
        self.map=new;self.map_source=source.into();self.map_at=Instant::now();true
    }
    pub fn current_map(&self)->&str{if self.map_at.elapsed()<Duration::from_secs(15){&self.map}else{""}}
    pub fn snapshot(&self)->Snapshot{
        let mut s=self.snap.clone();
        if self.map_at.elapsed()<Duration::from_secs(15){s.game_map=self.map.clone();s.game_map_source=self.map_source.clone();}
        s
    }
}