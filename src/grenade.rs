use crate::model::{Snapshot,GrenadeView};
use std::{collections::HashMap,time::{Duration,Instant}};
#[derive(Default)]pub struct GrenadeTracker{tracks:HashMap<u32,Trail>,map:String,pid:u32}
struct Trail{kind:String,points:Vec<[f32;3]>,last:Instant,active_at:Option<Instant>}
impl GrenadeTracker{
 pub fn reset(&mut self){self.tracks.clear();self.map.clear();self.pid=0;}
 pub fn update(&mut self,s:&mut Snapshot,now:Instant,map:&str){
  if (self.pid!=0&&self.pid!=s.pid)||(!self.map.is_empty()&&!map.is_empty()&&self.map!=map){self.reset();}
  self.pid=s.pid;if !map.is_empty(){self.map=map.into();}
  if !s.grenades_read{return;}
  for g in &mut s.grenades{
   if g.state=="flying"{
    let t=self.tracks.entry(g.id).or_insert_with(||Trail{kind:g.kind.clone(),points:vec![],last:now,active_at:None});
    let far=t.points.last().is_some_and(|p|distance(*p,g.position)>1600.);
    if t.kind!=g.kind||far{t.kind=g.kind.clone();t.points.clear();t.active_at=None;}
    if !g.trajectory.is_empty(){t.points=g.trajectory.iter().copied()
        .filter(|p|p.iter().all(|x|x.is_finite()&&x.abs()<100000.)).take(64).collect();}
    if t.points.last().is_none_or(|p|distance(*p,g.position)>8.){t.points.push(g.position);}
    if t.points.len()>40{t.points.drain(0..t.points.len()-40);}
    t.last=now;g.trajectory=t.points.clone();
   }else if let Some(t)=self.tracks.get_mut(&g.id){
    if t.kind==g.kind{
      let start=*t.active_at.get_or_insert(now);
      if now.saturating_duration_since(start)<Duration::from_millis(1200){g.trajectory=t.points.clone();}
      t.last=now;
    }
   }
  }
  let ids:std::collections::HashSet<u32>=s.grenades.iter().map(|v|v.id).collect();
  // Preserve a completed *observed* path for 700 ms after detonation.
  for (&id,t) in self.tracks.iter(){
   if !ids.contains(&id) && t.points.len()>1 && now.saturating_duration_since(t.last)<Duration::from_millis(700){
    s.grenades.push(GrenadeView{id,kind:t.kind.clone(),state:"finished".into(),
      position:*t.points.last().unwrap(),radius_units:0.,trajectory:t.points.clone(),fire_cells:vec![]});
   }
  }
  self.tracks.retain(|id,t|ids.contains(id)||now.saturating_duration_since(t.last)<Duration::from_millis(700));
 }
}
fn distance(a:[f32;3],b:[f32;3])->f32{((a[0]-b[0]).powi(2)+(a[1]-b[1]).powi(2)+(a[2]-b[2]).powi(2)).sqrt()}
#[cfg(test)]mod tests{
 use super::*;use crate::model::demo_snapshot;
 #[test]fn follows_observed_position(){
  let mut t=GrenadeTracker::default();let start=Instant::now();let mut s=demo_snapshot(0.);s.grenades_read=true;
  let i=s.grenades.iter().position(|g|g.state=="flying").unwrap();s.grenades[i].trajectory.clear();
  t.update(&mut s,start,"de_mirage");assert_eq!(s.grenades[i].trajectory.len(),1);
  s.grenades[i].position[0]+=100.;s.grenades[i].trajectory.clear();
  t.update(&mut s,start+Duration::from_millis(280),"de_mirage");assert_eq!(s.grenades[i].trajectory.len(),2);
 }
 #[test]fn does_not_predict_on_fast_frame(){let mut t=GrenadeTracker::default();let mut s=demo_snapshot(0.);
  let copy=s.grenades.clone();s.grenades_read=false;t.update(&mut s,Instant::now(),"de_mirage");assert_eq!(s.grenades,copy);}
 #[test]fn finished_trail_expires(){
  let mut t=GrenadeTracker::default();let at=Instant::now();let mut s=demo_snapshot(0.);s.grenades_read=true;
  let id=s.grenades.iter().find(|g|g.state=="flying").unwrap().id;
  t.update(&mut s,at,"de_mirage");
  s.grenades.clear();t.update(&mut s,at+Duration::from_millis(100),"de_mirage");
  assert!(s.grenades.iter().any(|g|g.id==id&&g.state=="finished"));
  s.grenades.clear();t.update(&mut s,at+Duration::from_millis(850),"de_mirage");
  assert!(s.grenades.is_empty());
 }
 #[test]fn reset_on_map_change(){
  let mut t=GrenadeTracker::default();let start=Instant::now();let mut s=demo_snapshot(0.);s.grenades_read=true;
  let i=s.grenades.iter().position(|g|g.state=="flying").unwrap();s.grenades[i].trajectory.clear();
  t.update(&mut s,start,"de_mirage");s.grenades[i].position[0]+=100.;s.grenades[i].trajectory.clear();
  t.update(&mut s,start+Duration::from_millis(280),"de_nuke");assert_eq!(s.grenades[i].trajectory.len(),1);
 }
}
