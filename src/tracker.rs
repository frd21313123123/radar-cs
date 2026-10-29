use crate::model::{BombWorld,Snapshot,unique_bomb_carrier};
use std::time::{Instant,Duration};

/// Tracks a confirmed, living T carrier. Unknown read != dropped C4.
#[derive(Default)]
pub struct BombTracker {
    carrier:Option<u32>,last_confirmed:Option<Instant>,negative_since:Option<Instant>,map:Option<String>,
    world:Option<(Instant,BombWorld)>,pid:u32,
}
impl BombTracker {
    pub fn reset(&mut self){*self=Self::default();}
    pub fn update(&mut self,s:&mut Snapshot,now:Instant,map:&str){
        if (self.pid!=0&&self.pid!=s.pid)
            || (self.map.as_deref().is_some_and(|m|m!=map)&&!map.is_empty()){self.reset();}
        self.pid=s.pid;
        if !map.is_empty(){self.map=Some(map.into());}
        let by_id=|id:u32| s.players.iter().find(|p|p.id==id);
        if s.details_read {
            if let Some(world)=s.bomb_world.as_ref() {
                self.world=Some((now,world.clone()));
                self.carrier=None;self.negative_since=None;
            } else if s.c4_global_owner_id.is_some() || unique_bomb_carrier(&s.players).is_some() {
                self.world=None;
            }
        }
        if s.bomb_world.is_none(){
            if let Some((at,world))=self.world.as_ref(){
                if now.saturating_duration_since(*at)<Duration::from_millis(1400) {
                    let mut w=world.clone();
                    if !s.details_read{w.source="recent".into();}
                    s.bomb_world=Some(w);
                } else {self.world=None;}
            }
        }
        let confirmed=if s.bomb_world.is_some(){None}else if s.details_read {
            s.c4_global_owner_id.or_else(||if !s.c4_unowned {unique_bomb_carrier(&s.players)} else {None})
        }else{None};
        if s.bomb_world.is_some() {
            self.carrier=None;self.negative_since=None;
        } else if let Some(id)=confirmed {
            self.world=None;
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
    #[test]fn a_new_process_does_not_inherit_the_previous_carrier(){
        let mut tracker=BombTracker::default();let at=Instant::now();
        let mut old=demo_snapshot(0.);old.pid=100;old.details_read=true;
        tracker.update(&mut old,at,"de_mirage");assert_eq!(old.bomb_carrier_id,Some(2));
        let mut new=demo_snapshot(0.);new.pid=101;new.details_read=true;
        for p in &mut new.players{p.has_c4=Some(false);}
        new.bomb_carrier_id=None;
        tracker.update(&mut new,at+Duration::from_millis(100),"de_mirage");
        assert!(new.bomb_carrier_id.is_none());
    }
    #[test]fn a_new_process_does_not_inherit_the_previous_world_marker(){
        let mut tracker=BombTracker::default();let at=Instant::now();
        let mut old=demo_snapshot(0.);old.pid=100;old.details_read=true;
        old.bomb_world=Some(BombWorld{position:[100.,200.,0.],status:"dropped".into(),source:"entity".into()});
        tracker.update(&mut old,at,"de_mirage");
        let mut new=demo_snapshot(0.);new.pid=101;new.details_read=false;
        tracker.update(&mut new,at+Duration::from_millis(100),"de_mirage");
        assert!(new.bomb_world.is_none());
    }
    #[test]fn switch_weapon_keeps_bomb(){
        let mut tr=BombTracker::default();let at=Instant::now();let mut snap=demo_snapshot(1.);
        snap.details_read=true;tr.update(&mut snap,at,"de_mirage");assert_eq!(snap.bomb_carrier_id,Some(2));
        let mut next=demo_snapshot(2.);next.details_read=false;next.players[1].weapon="AWP".into();next.players[1].has_c4=None;
        tr.update(&mut next,at+Duration::from_millis(200),"de_mirage");assert_eq!(next.bomb_carrier_id,Some(2));
    }
    #[test]fn unowned_clears(){let mut tr=BombTracker::default();let at=Instant::now();let mut s=demo_snapshot(1.);s.details_read=true;tr.update(&mut s,at,"de_mirage");s.c4_unowned=true;tr.update(&mut s,at+Duration::from_secs(1),"de_mirage");assert_eq!(s.bomb_carrier_id,None);}
    #[test]fn dropped_marker_overrides_previous_carrier(){
        let mut t=BombTracker::default();let at=Instant::now();let mut s=demo_snapshot(1.);
        s.details_read=true;t.update(&mut s,at,"de_mirage");assert_eq!(s.bomb_carrier_id,Some(2));
        let mut drop=demo_snapshot(2.);drop.details_read=true;
        drop.bomb_world=Some(BombWorld{position:[100.,200.,0.],status:"dropped".into(),source:"entity".into()});
        t.update(&mut drop,at+Duration::from_millis(350),"de_mirage");
        assert!(drop.bomb_carrier_id.is_none());
        assert_eq!(drop.bomb_world.as_ref().unwrap().position[0],100.);
        let mut quick=demo_snapshot(3.);quick.details_read=false;
        t.update(&mut quick,at+Duration::from_millis(550),"de_mirage");
        assert_eq!(quick.bomb_world.as_ref().unwrap().source,"recent");
        assert!(quick.bomb_carrier_id.is_none());
    }
    #[test]fn pickup_removes_world_marker(){
        let mut t=BombTracker::default();let at=Instant::now();let mut s=demo_snapshot(1.);
        s.details_read=true;s.bomb_world=Some(BombWorld{position:[100.,200.,0.],status:"dropped".into(),source:"entity".into()});
        t.update(&mut s,at,"de_mirage");
        let mut carried=demo_snapshot(2.);carried.details_read=true;
        t.update(&mut carried,at+Duration::from_millis(350),"de_mirage");
        assert!(carried.bomb_world.is_none());assert_eq!(carried.bomb_carrier_id,Some(2));
    }
    #[test]fn dropped_marker_expires_without_fresh_data(){
        let mut t=BombTracker::default();let at=Instant::now();let mut s=demo_snapshot(1.);
        s.details_read=true;s.bomb_world=Some(BombWorld{position:[100.,200.,0.],status:"dropped".into(),source:"entity".into()});
        t.update(&mut s,at,"de_mirage");
        let mut missing=demo_snapshot(2.);missing.details_read=false;
        t.update(&mut missing,at+Duration::from_millis(2000),"de_mirage");
        assert!(missing.bomb_world.is_none());
    }
}