use crate::model::{LocalMotion,NoiseState,Snapshot};
use std::time::{Duration,Instant};

// Estimates only. Walls, surfaces, floors and CS2 sound-occlusion are unavailable.
#[derive(Default)]
pub struct NoiseTracker {
    ground:Option<bool>,shots:Option<i32>,weapon:Option<String>,
    event:Option<(Instant,&'static str,f32)>,pid:u32,map:String,
}
impl NoiseTracker {
    pub fn reset(&mut self){*self=Self::default();}
    pub fn update(&mut self,s:&mut Snapshot,now:Instant,map:&str){
        if (self.pid!=0&&self.pid!=s.pid)||(!self.map.is_empty()&&!map.is_empty()&&self.map!=map){self.reset();}
        self.pid=s.pid;if !map.is_empty(){self.map=map.into();}
        s.noise=None;
        let m=match s.local_motion.as_ref(){Some(v)=>v,None=>{self.ground=None;self.shots=None;self.weapon=None;self.event=None;return;}};
        let speed=m.speed.filter(|v|v.is_finite()&&*v>=0.&&*v<2000.).unwrap_or(0.);
        if let (Some(old),Some(new))=(self.ground,m.on_ground){
            if !old&&new{self.event=Some((now,"landing",1100.));}
            else if old&&!new{self.event=Some((now,"jump",900.));}
        }
        if let (Some(old),Some(new))=(self.shots,m.shots_fired){
            if new>old&&new>0{
                let silent=matches!(m.weapon.as_deref(),Some("M4A1-S"|"USP-S"));
                self.event=Some((now,if silent{"silenced_shot"}else{"shot"},if silent{1200.}else{2800.}));
            }
        }
        if let (Some(old),Some(new))=(self.weapon.as_deref(),m.weapon.as_deref()){
            if old!=new && !new.is_empty() && new!="—" && !self.event.as_ref().is_some_and(|(at,_,_)|*at==now){
                self.event=Some((now,"weapon_switch",350.));
            }
        }
        if m.on_ground.is_some(){self.ground=m.on_ground;}
        if m.shots_fired.is_some(){self.shots=m.shots_fired;}
        if let Some(w)=m.weapon.as_deref().filter(|x|!x.is_empty()&&*x!="—"){self.weapon=Some(w.into());}
        if let Some((at,kind,radius))=self.event{
            let age=now.saturating_duration_since(at);
            if age<Duration::from_millis(if kind=="landing"{520}else{480}){
                s.noise=Some(NoiseState{kind:kind.into(),radius_units:radius,approximate:true,age_ms:age.as_millis() as u32});
                return;
            }
        }
        self.event=None;
        if speed>=150. && m.on_ground!=Some(false)&&m.ducking!=Some(true){
            s.noise=Some(NoiseState{kind:"running".into(),radius_units:900.,approximate:true,age_ms:0});
        }
    }
}
#[cfg(test)]mod tests{
    use super::*;use crate::model::demo_snapshot;
    fn sample(speed:f32,ground:bool,duck:bool,shots:i32,weapon:&str)->Snapshot{
        let mut s=demo_snapshot(0.);
        s.local_motion=Some(LocalMotion{speed:Some(speed),on_ground:Some(ground),ducking:Some(duck),shots_fired:Some(shots),weapon:Some(weapon.into())});s
    }
    #[test]fn walk_crouch_silent(){
        let t=Instant::now();let mut n=NoiseTracker::default();
        let mut s=sample(100.,true,false,0,"AK-47");n.update(&mut s,t,"de_mirage");assert!(s.noise.is_none());
        let mut s=sample(220.,true,true,0,"AK-47");n.update(&mut s,t+Duration::from_millis(10),"de_mirage");assert!(s.noise.is_none());
    }
    #[test]fn run_and_land(){
        let t=Instant::now();let mut n=NoiseTracker::default();
        let mut s=sample(230.,true,false,0,"AK-47");n.update(&mut s,t,"de_mirage");assert_eq!(s.noise.unwrap().kind,"running");
        let mut s=sample(220.,false,false,0,"AK-47");n.update(&mut s,t+Duration::from_millis(20),"de_mirage");assert_eq!(s.noise.unwrap().kind,"jump");
        let mut s=sample(0.,true,false,0,"AK-47");n.update(&mut s,t+Duration::from_millis(50),"de_mirage");assert_eq!(s.noise.unwrap().kind,"landing");
    }
    #[test]fn shot_and_timeout(){
        let t=Instant::now();let mut n=NoiseTracker::default();
        let mut s=sample(0.,true,false,0,"USP-S");n.update(&mut s,t,"de_mirage");
        let mut s=sample(0.,true,false,1,"USP-S");n.update(&mut s,t+Duration::from_millis(15),"de_mirage");assert_eq!(s.noise.unwrap().radius_units,1200.);
        let mut s=sample(0.,true,false,1,"USP-S");n.update(&mut s,t+Duration::from_millis(700),"de_mirage");assert!(s.noise.is_none());
    }
    #[test]fn no_local_data_no_sound(){
        let t=Instant::now();let mut n=NoiseTracker::default();
        let mut s=sample(250.,true,false,0,"AK-47");n.update(&mut s,t,"de_mirage");
        s.local_motion=None;n.update(&mut s,t+Duration::from_millis(20),"de_mirage");assert!(s.noise.is_none());
    }
}