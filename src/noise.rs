use crate::model::{LocalMotion,NoiseState,Snapshot};
use std::{collections::VecDeque,time::{Duration,Instant}};

// Game audio curves, not arbitrary speed-dependent disks. These open-space
// bounds do not model walls/occlusion; provenance: assets/sound_profiles.md.
pub const FOOTSTEP_RADIUS:f32=1100.;
pub const SILENCED_RADIUS:f32=1400.;
pub const METRES_PER_UNIT:f32=0.0254;

#[derive(Clone,Copy,Debug)]pub struct NoisePulse{pub radius_fraction:f32,pub opacity:f32}
pub fn pulse_at(age_ms:f32,duration_ms:u32)->Option<NoisePulse>{
    if !age_ms.is_finite()||age_ms<0.||duration_ms==0||age_ms>=duration_ms as f32{return None;}
    let phase=(age_ms/duration_ms as f32).clamp(0.,1.);
    Some(NoisePulse{radius_fraction:1.-(1.-phase).powi(3),opacity:(1.-phase).powf(1.5)})
}
fn increased(old:Option<f32>,new:Option<f32>)->bool{
    matches!((old,new),(Some(a),Some(b)) if a.is_finite()&&b.is_finite()&&b>a+0.0001)
}
fn shot_radius(m:&LocalMotion)->Option<f32>{
    let weapon=m.weapon.as_deref()?;
    if weapon=="MP5-SD"||(matches!(weapon,"M4A1-S"|"USP-S")&&m.silenced==Some(true)){return Some(SILENCED_RADIUS);}
    if weapon=="USP-S"&&m.silenced==Some(false){return Some(2500.);}
    if weapon=="Zeus x27"{return Some(2420.2856);}
    if matches!(weapon,"AK-47"|"M4A4"|"M4A1-S"|"USP-S"|"AWP"|"SSG 08"|"SCAR-20"|"G3SG1"|
        "AUG"|"FAMAS"|"Galil AR"|"SG 553"|"Glock-18"|"P2000"|"P250"|"Five-SeveN"|"Tec-9"|
        "CZ75-Auto"|"Desert Eagle"|"Dual Berettas"|"R8 Revolver"|"MAC-10"|"MP9"|"MP7"|"MP5-SD"|
        "P90"|"PP-Bizon"|"UMP-45"|"Nova"|"XM1014"|"MAG-7"|"Sawed-Off"|"M249"|"Negev"){
        // The near-shot curve is not a cutoff: distant child sounds continue.
        // This is the game's broadcast upper bound, not guaranteed audibility.
        Some(9000.)
    }else{None}
}

#[derive(Default)]pub struct NoiseTracker{
    previous:Option<LocalMotion>,last_position:Option<[f32;3]>,last_step:Option<Instant>,
    distance:f32,events:VecDeque<(Instant,NoiseState)>,next_id:u64,pid:u32,map:String,local_id:Option<u32>,
}
impl NoiseTracker{
    pub fn reset(&mut self){*self=Self::default();}
    fn emit(&mut self,now:Instant,kind:&str,radius:f32,position:[f32;3]){
        let debounce=if kind=="footstep"{140}else{35};
        if self.events.iter().rev().any(|(at,event)|event.kind==kind&&now.saturating_duration_since(*at)<Duration::from_millis(debounce)){return;}
        self.next_id=self.next_id.wrapping_add(1);
        self.events.push_back((now,NoiseState{kind:kind.into(),radius_units:radius,approximate:true,
            age_ms:0,event_id:self.next_id,position,duration_ms:if kind.contains("shot"){1200}else{1000}}));
        while self.events.len()>8{self.events.pop_front();}
        if kind=="footstep"{self.last_step=Some(now);self.distance=0.;}
    }
    pub fn update(&mut self,s:&mut Snapshot,now:Instant,map:&str){
        let local=s.players.iter().find(|p|p.local&&p.life_state=="alive");
        let id=local.map(|p|p.id);
        if (self.pid!=0&&self.pid!=s.pid)||(!self.map.is_empty()&&!map.is_empty()&&self.map!=map)||self.local_id!=id{self.reset();}
        self.pid=s.pid;self.local_id=id;if !map.is_empty(){self.map=map.into();}
        s.noise=None;s.noise_events.clear();
        let (Some(player),Some(m))=(local,s.local_motion.as_ref()) else{
            self.previous=None;self.last_position=None;self.last_step=None;self.distance=0.;self.events.clear();return;
        };
        let position=player.position;
        if !position.iter().all(|v|v.is_finite()){self.reset();return;}
        let speed=m.speed.filter(|v|v.is_finite()&&(0.0..2000.).contains(v)).unwrap_or(0.);
        let moved=self.last_position.map(|old|(old[0]-position[0]).hypot(old[1]-position[1])).unwrap_or(0.);
        if moved>256.{self.previous=None;self.distance=0.;self.last_step=Some(now);}
        let mut event=None;
        if let Some(old)=self.previous.as_ref(){
            let shot=increased(old.shot_time,m.shot_time)||
                (old.weapon==m.weapon&&matches!((old.shots_fired,m.shots_fired),(Some(a),Some(b)) if b>a&&b>0));
            let emitted=increased(old.emit_sound_time,m.emit_sound_time);
            let landed=old.on_ground==Some(false)&&m.on_ground==Some(true);
            let impact=(-old.vertical_speed.unwrap_or(0.)).max(0.).max(old.fall_velocity.unwrap_or(0.).max(0.));
            let can_step=m.on_ground==Some(true)&&m.walking!=Some(true)&&m.ducking!=Some(true);
            if shot{
                if let Some(radius)=shot_radius(m){event=Some((if radius==SILENCED_RADIUS{"silenced_shot"}else{"shot"},radius));}
            }
            if event.is_none()&&landed&&impact>=140.&&(emitted||impact>=220.){event=Some(("landing",FOOTSTEP_RADIUS));}
            if event.is_none()&&old.on_ground==Some(true)&&m.on_ground==Some(false)&&
                m.vertical_speed.is_some_and(|v|v>100.)&&emitted{event=Some(("jump",98.));}
            if event.is_none()&&can_step{
                if speed>=135.{self.distance+=moved;}else{self.distance=0.;}
                let side_changed=matches!((old.step_side,m.step_side),(Some(a),Some(b)) if a!=b);
                let timer_reset=matches!((old.step_sound_time,m.step_sound_time),(Some(a),Some(b)) if b>a+30.);
                let observed=if m.step_side.is_some(){side_changed}else if m.emit_sound_time.is_some(){emitted&&speed>5.}else{timer_reset&&speed>5.};
                let has_game_signal=m.step_side.is_some()||m.emit_sound_time.is_some()||m.step_sound_time.is_some();
                let fallback=speed>=135.&&!has_game_signal&&self.distance>=100.&&self.last_step
                    .is_some_and(|at|now.saturating_duration_since(at)>=Duration::from_millis(280));
                if observed||fallback{event=Some(("footstep",FOOTSTEP_RADIUS));}
            }else if !can_step{self.distance=0.;}
        }
        if self.last_step.is_none(){self.last_step=Some(now);}
        self.previous=Some(m.clone());self.last_position=Some(position);
        if let Some((kind,radius))=event{self.emit(now,kind,radius,position);}
        self.events.retain(|(at,event)|now.saturating_duration_since(*at)<Duration::from_millis(event.duration_ms as u64));
        for (at,event) in &self.events{
            let mut event=event.clone();event.age_ms=now.saturating_duration_since(*at).as_millis() as u32;
            s.noise_events.push(event);
        }
        s.noise=s.noise_events.last().cloned();
    }
}

#[cfg(test)]mod tests{
    use super::*;use crate::model::demo_snapshot;
    fn sample()->Snapshot{
        let mut s=demo_snapshot(0.);s.local_motion=Some(LocalMotion{speed:Some(230.),on_ground:Some(true),
            ducking:Some(false),walking:Some(false),step_side:Some(0),emit_sound_time:Some(10.),
            shots_fired:Some(0),weapon:Some("AK-47".into()),..LocalMotion::default()});s
    }
    #[test]fn a_velocity_alone_is_not_a_continuous_sound(){
        let mut t=NoiseTracker::default();let now=Instant::now();let mut s=sample();
        t.update(&mut s,now,"de_nuke");assert!(s.noise_events.is_empty());
        t.update(&mut s,now+Duration::from_millis(500),"de_nuke");assert!(s.noise_events.is_empty());
    }
    #[test]fn an_observed_step_is_not_discarded_by_a_fixed_speed_threshold(){
        let mut tracker=NoiseTracker::default();let at=Instant::now();let mut s=sample();
        s.local_motion.as_mut().unwrap().speed=Some(100.);tracker.update(&mut s,at,"de_nuke");
        s.local_motion.as_mut().unwrap().step_side=Some(1);tracker.update(&mut s,at+Duration::from_millis(400),"de_nuke");
        assert_eq!(s.noise.as_ref().unwrap().kind,"footstep");
    }
    #[test]fn actual_steps_produce_stationary_independent_pulses(){
        let mut t=NoiseTracker::default();let now=Instant::now();let mut s=sample();
        t.update(&mut s,now,"de_nuke");s.local_motion.as_mut().unwrap().step_side=Some(1);
        t.update(&mut s,now+Duration::from_millis(400),"de_nuke");assert_eq!(s.noise_events.len(),1);
        let origin=s.noise_events[0].position;assert_eq!(s.noise_events[0].radius_units,1100.);
        s.players.iter_mut().find(|p|p.local).unwrap().position[0]+=80.;
        s.local_motion.as_mut().unwrap().step_side=Some(0);
        t.update(&mut s,now+Duration::from_millis(800),"de_nuke");
        assert_eq!(s.noise_events.len(),2);assert_eq!(s.noise_events[0].position,origin);
        assert_ne!(s.noise_events[0].position,s.noise_events[1].position);
        t.update(&mut s,now+Duration::from_secs(2),"de_nuke");assert!(s.noise_events.is_empty());
    }
    #[test]fn walk_crouch_air_and_unknown_ground_are_not_running_sounds(){
        for mode in 0..4{
            let mut t=NoiseTracker::default();let now=Instant::now();let mut s=sample();t.update(&mut s,now,"de_nuke");
            let m=s.local_motion.as_mut().unwrap();m.step_side=Some(1);
            match mode{0=>m.walking=Some(true),1=>m.ducking=Some(true),2=>m.on_ground=Some(false),_=>m.on_ground=None}
            t.update(&mut s,now+Duration::from_millis(400),"de_nuke");assert!(s.noise_events.is_empty());
        }
    }
    #[test]fn a_suppressed_weapon_uses_its_actual_silencer_state(){
        for silenced in [true,false]{
            let mut t=NoiseTracker::default();let now=Instant::now();let mut s=sample();
            let m=s.local_motion.as_mut().unwrap();m.weapon=Some("USP-S".into());m.silenced=Some(silenced);
            t.update(&mut s,now,"de_nuke");s.local_motion.as_mut().unwrap().shots_fired=Some(1);
            t.update(&mut s,now+Duration::from_millis(100),"de_nuke");
            assert_eq!(s.noise.as_ref().unwrap().radius_units,if silenced{1400.}else{2500.});
        }
    }
    #[test]fn switching_weapon_or_ground_flags_does_not_invent_sound(){
        let mut t=NoiseTracker::default();let now=Instant::now();let mut s=sample();t.update(&mut s,now,"de_nuke");
        s.local_motion.as_mut().unwrap().weapon=Some("Knife".into());s.local_motion.as_mut().unwrap().on_ground=Some(false);
        t.update(&mut s,now+Duration::from_millis(20),"de_nuke");
        s.local_motion.as_mut().unwrap().on_ground=Some(true);
        t.update(&mut s,now+Duration::from_millis(40),"de_nuke");assert!(s.noise_events.is_empty());
    }
    #[test]fn fallback_requires_travel_instead_of_a_speed_flag(){
        let mut t=NoiseTracker::default();let now=Instant::now();let mut s=sample();
        let m=s.local_motion.as_mut().unwrap();m.step_side=None;m.emit_sound_time=None;
        t.update(&mut s,now,"de_nuke");s.players.iter_mut().find(|p|p.local).unwrap().position[0]+=110.;
        t.update(&mut s,now+Duration::from_millis(400),"de_nuke");assert_eq!(s.noise.as_ref().unwrap().kind,"footstep");
    }
    #[test]fn a_hard_landing_and_confirmed_jump_have_distinct_profiles(){
        let mut tracker=NoiseTracker::default();let at=Instant::now();let mut s=sample();
        tracker.update(&mut s,at,"de_nuke");
        let m=s.local_motion.as_mut().unwrap();m.on_ground=Some(false);m.vertical_speed=Some(300.);m.emit_sound_time=Some(10.1);
        tracker.update(&mut s,at+Duration::from_millis(100),"de_nuke");assert_eq!(s.noise.as_ref().unwrap().radius_units,98.);
        s.local_motion.as_mut().unwrap().vertical_speed=Some(-300.);
        tracker.update(&mut s,at+Duration::from_millis(400),"de_nuke");
        s.local_motion.as_mut().unwrap().on_ground=Some(true);s.local_motion.as_mut().unwrap().vertical_speed=Some(0.);
        tracker.update(&mut s,at+Duration::from_millis(500),"de_nuke");
        assert_eq!(s.noise.as_ref().unwrap().kind,"landing");assert_eq!(s.noise.as_ref().unwrap().radius_units,1100.);
    }
    #[test]fn air_to_ground_while_moving_up_is_not_a_landing(){
        let mut tracker=NoiseTracker::default();let at=Instant::now();let mut s=sample();
        s.local_motion.as_mut().unwrap().on_ground=Some(false);s.local_motion.as_mut().unwrap().vertical_speed=Some(300.);
        tracker.update(&mut s,at,"de_nuke");
        s.local_motion.as_mut().unwrap().on_ground=Some(true);s.local_motion.as_mut().unwrap().vertical_speed=Some(0.);
        tracker.update(&mut s,at+Duration::from_millis(100),"de_nuke");assert!(s.noise_events.is_empty());
    }
    #[test]fn respawn_and_missing_local_data_clear_pending_sounds(){
        let mut t=NoiseTracker::default();let now=Instant::now();let mut s=sample();t.update(&mut s,now,"de_nuke");
        s.local_motion.as_mut().unwrap().step_side=Some(1);t.update(&mut s,now+Duration::from_millis(400),"de_nuke");
        s.players.iter_mut().find(|p|p.local).unwrap().life_state="dead".into();
        t.update(&mut s,now+Duration::from_millis(500),"de_nuke");assert!(s.noise_events.is_empty());
    }
    #[test]fn animation_advances_fades_and_expires(){
        let first=pulse_at(100.,1000).unwrap();let later=pulse_at(700.,1000).unwrap();
        assert!(first.radius_fraction<later.radius_fraction&&first.opacity>later.opacity);
        assert!(later.radius_fraction<=1.);assert!(pulse_at(1000.,1000).is_none());
        assert!(pulse_at(f32::NAN,1000).is_none());assert!(pulse_at(-1.,1000).is_none());
        assert!((FOOTSTEP_RADIUS*METRES_PER_UNIT-27.94).abs()<0.01);
    }
}
