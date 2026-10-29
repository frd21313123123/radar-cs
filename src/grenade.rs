use crate::model::{Snapshot,GrenadeView};
use std::{collections::HashMap,time::{Duration,Instant}};
#[derive(Default)]pub struct GrenadeTracker{tracks:HashMap<u32,Trail>,map:String,pid:u32}
struct Trail{kind:String,instance:Option<(u64,u32)>,missing:bool,points:Vec<[f32;3]>,last:Instant,active_at:Option<Instant>,position:[f32;3],timer_deadline:Option<Instant>,timer_estimated:bool}
impl Trail{
 fn new(g:&GrenadeView,now:Instant)->Self{Self{kind:g.kind.clone(),instance:g.entity_instance,missing:false,
  points:vec![],last:now,active_at:None,position:g.position,timer_deadline:None,timer_estimated:false}}
}
impl GrenadeTracker{
 pub fn reset(&mut self){self.tracks.clear();self.map.clear();self.pid=0;}
 pub fn update(&mut self,s:&mut Snapshot,now:Instant,map:&str){
  if (self.pid!=0&&self.pid!=s.pid)||(!self.map.is_empty()&&!map.is_empty()&&self.map!=map){self.reset();}
  self.pid=s.pid;if !map.is_empty(){self.map=map.into();}
  if !s.grenades_read{return;}
  for g in &mut s.grenades{
   let reused=self.tracks.get(&g.id).is_some_and(|t|
    t.instance!=g.entity_instance ||
    // Legacy/demo records lack a generation. An expired estimated effect
    // observed missing in a scan must not give a new activation a zero timer.
    (g.entity_instance.is_none()&&t.missing&&t.timer_estimated&&
     t.timer_deadline.is_some_and(|deadline|now>=deadline)));
   if reused{self.tracks.remove(&g.id);}
   if g.state=="flying"{
    let t=self.tracks.entry(g.id).or_insert_with(||Trail::new(g,now));
    let far=t.points.last().is_some_and(|p|distance(*p,g.position)>1600.);
    if t.kind!=g.kind||far||t.active_at.is_some(){t.kind=g.kind.clone();t.points.clear();t.active_at=None;t.timer_deadline=None;}
    if g.trajectory.len()>=2 {
      // Merge with sampled positions instead of replacing existing tracks
      // with a shorter/broken game history after every update.
      let incoming:Vec<[f32;3]>=g.trajectory.iter().copied()
        .filter(|p|p.iter().all(|x|x.is_finite()&&x.abs()<100000.)).take(64).collect();
      if incoming.len()>t.points.len() && incoming.last()
          .is_some_and(|last|distance(*last,g.position)<2000.) {
        t.points=incoming;
      }
    }
    if t.points.last().is_none_or(|p|distance(*p,g.position)>3.){t.points.push(g.position);}
    if t.points.len()>40{t.points.drain(0..t.points.len()-40);}
    t.last=now;t.position=g.position;g.trajectory=t.points.clone();
   }else if g.state=="active"{
    let t=self.tracks.entry(g.id).or_insert_with(||Trail::new(g,now));
    if t.kind!=g.kind || distance(t.position,g.position)>1600. {
      t.kind=g.kind.clone();t.points.clear();t.active_at=None;t.timer_deadline=None;
    }
    let start=*t.active_at.get_or_insert(now);
    let age=now.saturating_duration_since(start);
    if age<Duration::from_millis(1200){g.trajectory=t.points.clone();}
    if matches!(g.kind.as_str(),"smoke"|"molotov") {
      if let Some(left)=g.remaining_secs.filter(|v|v.is_finite()&&(0.0..=60.0).contains(v)) {
        t.timer_deadline=Some(now+Duration::from_secs_f32(left));t.timer_estimated=g.timer_estimated;
      }else if t.timer_deadline.is_none(){
        let lifetime=if g.kind=="smoke"{20.}else{7.};
        t.timer_deadline=Some(start+Duration::from_secs_f32(lifetime));
        // The effect may already have been active when first observed.
        t.timer_estimated=true;
      }
      if let Some(end)=t.timer_deadline {
        g.remaining_secs=Some(end.saturating_duration_since(now).as_secs_f32());
        g.timer_estimated=t.timer_estimated;
      }
    }
    t.last=now;t.position=g.position;
   }
  }
  let ids:std::collections::HashSet<u32>=s.grenades.iter().map(|v|v.id).collect();
  // Preserve a completed *observed* path for 700 ms after detonation.
  for (&id,t) in self.tracks.iter(){
   if !ids.contains(&id) && t.points.len()>1 && now.saturating_duration_since(t.last)<Duration::from_millis(700){
    s.grenades.push(GrenadeView{id,entity_instance:t.instance,kind:t.kind.clone(),state:"finished".into(),
      position:*t.points.last().unwrap(),radius_units:0.,trajectory:t.points.clone(),fire_cells:vec![],remaining_secs:None,timer_estimated:false});
   }
  }
  for (id,t) in &mut self.tracks{t.missing=!ids.contains(id);}
  // Retain activation clocks across transient missing reads; never restart
  // an estimated countdown simply because an entity was absent for a scan.
  self.tracks.retain(|id,t|ids.contains(id)||now.saturating_duration_since(t.last)<
    if t.active_at.is_some(){Duration::from_secs(25)}else{Duration::from_millis(700)});
 }
}
fn distance(a:[f32;3],b:[f32;3])->f32{((a[0]-b[0]).powi(2)+(a[1]-b[1]).powi(2)+(a[2]-b[2]).powi(2)).sqrt()}
#[cfg(test)]mod tests{
 use super::*;use crate::model::demo_snapshot;
 #[test]fn estimated_timer_survives_disappearance_without_restarting(){
  let mut t=GrenadeTracker::default();let at=Instant::now();let mut s=demo_snapshot(0.);
  s.grenades.retain(|g|g.kind=="smoke"&&g.state=="active");
  s.grenades[0].remaining_secs=None;t.update(&mut s,at,"de_mirage");
  assert_eq!(s.grenades[0].remaining_secs,Some(20.));assert!(s.grenades[0].timer_estimated);
  let grenade=s.grenades[0].clone();s.grenades.clear();
  t.update(&mut s,at+Duration::from_secs(2),"de_mirage");
  s.grenades=vec![grenade];s.grenades[0].remaining_secs=None;
  t.update(&mut s,at+Duration::from_secs(5),"de_mirage");
  assert_eq!(s.grenades[0].remaining_secs,Some(15.));
 }
 #[test]fn missing_clock_preserves_last_authoritative_expiry(){
  let mut t=GrenadeTracker::default();let at=Instant::now();let mut s=demo_snapshot(0.);
  s.grenades.retain(|g|g.kind=="molotov");
  s.grenades[0].remaining_secs=Some(2.);t.update(&mut s,at,"de_mirage");
  s.grenades[0].remaining_secs=None;t.update(&mut s,at+Duration::from_secs(1),"de_mirage");
  assert_eq!(s.grenades[0].remaining_secs,Some(1.));assert!(!s.grenades[0].timer_estimated);
  s.grenades[0].remaining_secs=None;t.update(&mut s,at+Duration::from_secs(3),"de_mirage");
  assert_eq!(s.grenades[0].remaining_secs,Some(0.));
 }
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
 #[test]fn existing_path_does_not_shrink(){
  let mut t=GrenadeTracker::default();let at=Instant::now();
  let mut s=demo_snapshot(0.);s.grenades_read=true;
  let i=s.grenades.iter().position(|g|g.state=="flying").unwrap();
  t.update(&mut s,at,"de_mirage");
  let n=s.grenades[i].trajectory.len();
  s.grenades[i].trajectory=vec![s.grenades[i].position];
  t.update(&mut s,at+Duration::from_millis(100),"de_mirage");
  assert!(s.grenades[i].trajectory.len()>=n);
 }
 #[test]fn reset_on_map_change(){
  let mut t=GrenadeTracker::default();let start=Instant::now();let mut s=demo_snapshot(0.);s.grenades_read=true;
  let i=s.grenades.iter().position(|g|g.state=="flying").unwrap();s.grenades[i].trajectory.clear();
  t.update(&mut s,start,"de_mirage");s.grenades[i].position[0]+=100.;s.grenades[i].trajectory.clear();
  t.update(&mut s,start+Duration::from_millis(280),"de_nuke");assert_eq!(s.grenades[i].trajectory.len(),1);
 }
 #[test]fn a_new_generation_at_the_same_slot_starts_a_new_countdown(){
  let mut tracker=GrenadeTracker::default();let at=Instant::now();let mut s=demo_snapshot(0.);
  s.grenades.retain(|g|g.kind=="molotov");s.grenades[0].entity_instance=Some((0x10000,0x8001));
  s.grenades[0].remaining_secs=None;tracker.update(&mut s,at,"de_nuke");
  s.grenades[0].entity_instance=Some((0x10000,0x10001));s.grenades[0].remaining_secs=None;
  tracker.update(&mut s,at+Duration::from_secs(5),"de_nuke");
  assert_eq!(s.grenades[0].remaining_secs,Some(7.));
 }
 #[test]fn reused_legacy_slot_after_an_expired_missing_effect_gets_a_new_timer(){
  let mut tracker=GrenadeTracker::default();let at=Instant::now();let mut s=demo_snapshot(0.);
  s.grenades.retain(|g|g.kind=="molotov");s.grenades[0].remaining_secs=None;
  tracker.update(&mut s,at,"de_nuke");let first=s.grenades.clone();
  s.grenades.clear();tracker.update(&mut s,at+Duration::from_secs(8),"de_nuke");
  s.grenades=first;s.grenades[0].remaining_secs=None;
  tracker.update(&mut s,at+Duration::from_secs(9),"de_nuke");
  assert_eq!(s.grenades[0].remaining_secs,Some(7.));
 }
 #[test]fn a_known_instance_keeps_its_clock_through_a_missing_scan(){
  let mut tracker=GrenadeTracker::default();let at=Instant::now();let mut s=demo_snapshot(0.);
  s.grenades.retain(|g|g.kind=="smoke"&&g.state=="active");
  s.grenades[0].entity_instance=Some((0x10000,0x8001));s.grenades[0].remaining_secs=None;
  tracker.update(&mut s,at,"de_nuke");let first=s.grenades.clone();
  s.grenades.clear();tracker.update(&mut s,at+Duration::from_secs(2),"de_nuke");
  s.grenades=first;s.grenades[0].remaining_secs=None;
  tracker.update(&mut s,at+Duration::from_secs(5),"de_nuke");
  assert_eq!(s.grenades[0].remaining_secs,Some(15.));
 }
}
