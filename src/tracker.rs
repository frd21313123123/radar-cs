use crate::model::{Snapshot,unique_bomb_carrier};
use std::time::{Instant,Duration};

/// Tracks a confirmed, living T carrier. Unknown read != dropped C4.
#[derive(Default)]
pub struct BombTracker {
    carrier:Option<u32>,last_confirmed:Option<Instant>,negative_since:Option<Instant>,map:Option<String>,
}
impl BombTracker {
    pub fn reset(&mut self){*self=Self::default();}
    pub fn update(&mut self,s:&mut Snapshot,now:Instant,map:&str){
        if self.map.as_deref().is_some_and(|m|m!=map) && !map.is_empty(){self.reset();}
        if !map.is_empty(){self.map=Some(map.into());}
        let by_id=|id:u32| s.players.iter().find(|p|p.id==id);
        let confirmed=if s.details_read {s.c4_global_owner_id.or_else(||if !s.c4_unowned {unique_bomb_carrier(&s.players)} else {None})}else{None};
        if let Some(id)=confirmed {
            self.carrier=Some(id);self.last_confirmed=Some(now);self.negative_since=None;
        } else if s.c4_unowned {
            self.carrier=None;self.negative_since=None;
        } else if let Some(id)=self.carrier {
            let old=by_id(id);
            if old.is_some_and(|p|p.team!=2 || p.life_state=="dead") {self.carrier=None;self.negative_since=None;}
            else if s.details_read && old.is_some_and(|p|p.has_c4==Some(false)) {
                let from=*self.negative_since.get_or_insert(now);
                if now.duration_since(from)>=Duration::from_millis(900){self.carrier=None;self.negative_since=None;}
            } else if old.is_some_and(|p|p.has_c4.is_none()){self.negative_since=None;}
            if self.last_confirmed.is_some_and(|at|now.duration_since(at)>Duration::from_secs(8)){self.carrier=None;self.negative_since=None;}
        }
        s.bomb_carrier_id=None;s.bomb_carrier_source=None;
        if let Some(id)=self.carrier {
            if let Some(p)=s.players.iter_mut().find(|p|p.id==id && p.team==2 && p.life_state=="alive") {
                let recent=self.last_confirmed.is_some_and(|at|now.duration_since(at)>Duration::from_millis(450));
                let source=if recent {"recent".to_owned()} else {p.c4_source.clone().unwrap_or_else(||"inventory".into())};
                p.has_c4=Some(true);p.c4_source=Some(source.clone());s.bomb_carrier_id=Some(id);s.bomb_carrier_source=Some(source);
            }
        }
    }
}

#[cfg(test)]mod tests{
    use super::*;use crate::model::demo_snapshot;
    #[test]fn switch_weapon_keeps_bomb(){
        let mut tr=BombTracker::default();let at=Instant::now();let mut snap=demo_snapshot(1.);
        snap.details_read=true;tr.update(&mut snap,at,"de_mirage");assert_eq!(snap.bomb_carrier_id,Some(2));
        let mut next=demo_snapshot(2.);next.details_read=false;next.players[1].weapon="AWP".into();next.players[1].has_c4=None;
        tr.update(&mut next,at+Duration::from_millis(200),"de_mirage");assert_eq!(next.bomb_carrier_id,Some(2));
    }
    #[test]fn unowned_clears(){let mut tr=BombTracker::default();let at=Instant::now();let mut s=demo_snapshot(1.);s.details_read=true;tr.update(&mut s,at,"de_mirage");s.c4_unowned=true;tr.update(&mut s,at+Duration::from_secs(1),"de_mirage");assert_eq!(s.bomb_carrier_id,None);}
}