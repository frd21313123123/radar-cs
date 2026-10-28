//! Source 2 read-only, best-effort controller/pawn reader.
use crate::model::{BombWorld, BombTimer, Diagnostics, GrenadeView, ObserverView, Player, Snapshot, LocalMotion, life_state, normalized_yaw, unique_bomb_carrier, now_secs, normalize_map_name};
use crate::process::Memory;
use crate::schema::Schema;
use serde_json::{json, Value};
use std::{collections::{HashMap,HashSet,BTreeMap},time::{Instant,Duration}};

fn weapon_name(id:u16)->String {
    let name=match id {1=>"Desert Eagle",2=>"Dual Berettas",3=>"Five-SeveN",4=>"Glock-18",7=>"AK-47",8=>"AUG",
    9=>"AWP",10=>"FAMAS",11=>"G3SG1",13=>"Galil AR",14=>"M249",16=>"M4A4",17=>"MAC-10",
    19=>"P90",23=>"MP5-SD",24=>"UMP-45",25=>"XM1014",26=>"PP-Bizon",27=>"MAG-7",
    28=>"Negev",29=>"Sawed-Off",30=>"Tec-9",31=>"Zeus x27",32=>"P2000",33=>"MP7",34=>"MP9",
    35=>"Nova",36=>"P250",38=>"SCAR-20",39=>"SG 553",40=>"SSG 08",42=>"Knife",43=>"Flashbang",
    44=>"HE Grenade",45=>"Smoke",46=>"Molotov",47=>"Decoy",48=>"Incendiary",49=>"C4",
    59=>"Knife",60=>"M4A1-S",61=>"USP-S",63=>"CZ75-Auto",64=>"R8 Revolver",_=>return format!("Item #{id}")};
    name.into()
}
fn field(s:&Schema,k:&str,n:&str)->Result<u64,String>{s.field(k,n).ok_or(format!("missing field {k}.{n}"))}
fn try_i32<M:Memory>(p:&mut M,addr:u64)->Option<i32>{p.read(addr,4).and_then(|v|v.try_into().ok()).map(i32::from_le_bytes)}
fn try_u8<M:Memory>(p:&mut M,addr:u64)->Option<u8>{p.read(addr,1).and_then(|v|v.first().copied())}
fn try_f32<M:Memory>(p:&mut M,addr:u64)->Option<f32>{
    p.read(addr,4).and_then(|bytes|bytes.try_into().ok())
        .map(f32::from_le_bytes).filter(|v|v.is_finite())
}
fn opt_u64<M:Memory>(p:&mut M,ptr:u64,offset:Option<u64>)->u64{if ptr>0{offset.map_or(0,|o|p.u64(ptr+o))}else{0}}
fn opt_i32<M:Memory>(p:&mut M,ptr:u64,offset:Option<u64>)->Option<i32>{if ptr>0{offset.and_then(|o|try_i32(p,ptr+o))}else{None}}

#[derive(Clone,Copy)] struct Layout{root:u64,chunk_offset:u64,stride:usize,confirmed:bool,slot:Option<u32>}
impl Layout {fn new(root:u64)->Self{Self{root,chunk_offset:0x10,stride:0x78,confirmed:false,slot:None}}}
pub struct CS2Reader {pub schema:Schema, layout:Layout, layout_at:Option<Instant>, probes:u32, chunk_cache:HashMap<u32,u64>}
impl CS2Reader {
    pub fn new(schema:Schema)->Self{Self{schema,layout:Layout::new(0),layout_at:None,probes:0,chunk_cache:HashMap::new()}}
    fn probe_layout<M:Memory>(&mut self,p:&mut M,list:u64,offset_addr:u64,local:u64){
        let now=Instant::now();
        if self.layout.confirmed && self.layout.root==list{return;}
        if self.layout.root!=0 && self.layout_at.is_some_and(|v|now.duration_since(v)<Duration::from_secs(3)){return;}
        self.layout_at=Some(now);self.probes=0;self.layout=Layout::new(list);
        if local==0{return;}
        let mut roots=vec![list,offset_addr,p.u64(list)]; roots.retain(|v|*v!=0);roots.sort_unstable();roots.dedup();
        for root in roots {for chunk_offset in [0x10,0x00,0x08,0x18,0x20] {
            let block=p.u64(root+chunk_offset);if block==0 {continue;}
            for stride in [0x78usize,0x70] {
                self.probes+=1;
                if let Some(raw)=p.read(block,stride*65) {
                    for slot in 1..=64u32 {
                        let at=stride*slot as usize;
                        if raw.get(at..at+8).and_then(|b|b.try_into().ok()).map(u64::from_le_bytes)==Some(local) {
                            self.layout=Layout{root,chunk_offset,stride,confirmed:true,slot:Some(slot)};return;
                        }
                    }
                }
            }
        }}
    }
    fn block<M:Memory>(&mut self,p:&mut M,list:u64,index:u32)->u64 {
        let chunk=index>>9;
        if let Some(&block)=self.chunk_cache.get(&chunk){return block;}
        let block=p.u64(list+self.layout.chunk_offset+8*chunk as u64);
        self.chunk_cache.insert(chunk,block);block
    }
    fn entity<M:Memory>(&mut self,p:&mut M,list:u64,index:u32)->u64 {
        if index>=0x8000 {return 0;}
        let block=self.block(p,list,index);if block==0{return 0;}
        p.u64(block+self.layout.stride as u64*(index & 0x1ff) as u64)
    }
    fn controllers<M:Memory>(&mut self,p:&mut M,list:u64,local:u64)->(Vec<(u32,u64)>,bool){
        let block=self.block(p,list,0);let mut out=Vec::new();
        if block!=0 {
            let stride=self.layout.stride;
            if let Some(data)=p.read(block+stride as u64,64*stride) {
                for slot in 1..=64u32 {let from=(slot-1) as usize*stride;
                    if let Some(ptr)=data.get(from..from+8).and_then(|b|b.try_into().ok()).map(u64::from_le_bytes){
                        if ptr!=0{out.push((slot,ptr));}
                    }
                }
            }
        }
        if out.is_empty(){for i in 1..=64 {let ptr=self.entity(p,list,i);if ptr!=0{out.push((i,ptr));}}}
        let fallback=local!=0 && !out.iter().any(|(_,ptr)|*ptr==local);
        if fallback{out.push((0,local));}
        (out,fallback)
    }
    fn item<M:Memory>(&self,p:&mut M,weapon:u64)->Option<(u16,String)>{
        if weapon==0{return None;}
        let addr=weapon+self.schema.field("C_EconEntity","m_AttributeManager")?
            +self.schema.field("C_AttributeContainer","m_Item")?
            +self.schema.field("C_EconItemView","m_iItemDefinitionIndex")?;
        let id=p.u16(addr);if id==0||id>=10000 {None}else{Some((id,weapon_name(id)))}
    }
    fn player_items<M:Memory>(&mut self,p:&mut M,list:u64,pawn:u64)->(String,Vec<String>,Option<bool>){
        let ptr=opt_u64(p,pawn,self.schema.field("C_BasePlayerPawn","m_pWeaponServices"));
        if ptr==0{return ("—".into(),vec![],None);}
        let active_off=self.schema.field("CPlayer_WeaponServices","m_hActiveWeapon");
        let h=active_off.map(|o|p.u32(ptr+o)).unwrap_or(0);
        let active=if h!=0 && h!=u32::MAX {self.entity(p,list,h&0x7fff)}else{0};
        let (active_id,active_name)=self.item(p,active).unwrap_or((0,"—".into()));
        let mut inventory=Vec::new();
        let vec_off=match self.schema.field("CPlayer_WeaponServices","m_hMyWeapons"){Some(o)=>o,None=>return(active_name,inventory,if active_id==49{Some(true)}else{None})};
        let vec=match p.read(ptr+vec_off,12){Some(v)=>v,None=>return(active_name.clone(),if active_id!=0{vec![active_name.clone()]}else{vec![]},if active_id==49{Some(true)}else{None})};
        let data_ptr=u64::from_le_bytes(vec[..8].try_into().unwrap());
        let count=u32::from_le_bytes(vec[8..12].try_into().unwrap()) as usize;
        if count>64||(count>0&&data_ptr==0){return(active_name,vec![],if active_id==49{Some(true)}else{None});}
        let handles=if count==0 {Some(vec![])} else {p.read(data_ptr,count*4)};
        let mut complete=count>0;
        let mut has_c4=active_id==49;
        match handles {
            None=>complete=false,
            Some(raw)=>{for i in 0..count{
                let h=u32::from_le_bytes(raw[4*i..4*i+4].try_into().unwrap());
                if h==0||h==u32::MAX ||h&0x7fff==0{complete=false;continue;}
                let w=self.entity(p,list,h&0x7fff);
                if let Some((id,name))=self.item(p,w){if id==49{has_c4=true;}if !inventory.contains(&name){inventory.push(name);}}
                else{complete=false;}
            }}
        }
        if active_id>0&&!inventory.contains(&active_name){inventory.push(active_name.clone());}
        (active_name,inventory,if has_c4{Some(true)}else if complete{Some(false)}else{None})
    }

    /// Use Source 2 game-time rather than "40 seconds since detected".
    /// GlobalVars.curtime is at +0x30 on the current CS2 client layout.
    /// An invalid/missing clock leaves the indicator hidden instead of
    /// showing an invented countdown.
    fn planted_c4_timer<M:Memory>(&self,p:&mut M,base:u64)->Option<BombTimer>{
        let ptr=p.u64(base+self.schema.offset("dwPlantedC4")?);
        if ptr<0x10000{return None;}
        let field=|name:&str|self.schema.field("C_PlantedC4",name);
        let ticking=field("m_bBombTicking").and_then(|o|try_u8(p,ptr+o))==Some(1);
        if !ticking{return None;}
        if field("m_bBombDefused").and_then(|o|try_u8(p,ptr+o))==Some(1)
            || field("m_bHasExploded").and_then(|o|try_u8(p,ptr+o))==Some(1) {
            return None;
        }
        let blow=try_f32(p,ptr+field("m_flC4Blow")?)?;
        let total=try_f32(p,ptr+field("m_flTimerLength")?)?;
        let gamevars=p.u64(base+self.schema.offset("dwGlobalVars")?);
        if gamevars<0x10000{return None;}
        // Read actual client game time. This field is not schema-reflected.
        let current=try_f32(p,gamevars+0x30)?;
        let site=field("m_nBombSite").and_then(|o|try_i32(p,ptr+o)).unwrap_or(-1);
        let defusing=field("m_bBeingDefused").and_then(|o|try_u8(p,ptr+o))==Some(1);
        BombTimer::from_game_time(site,blow,current,total,defusing)
    }
    /// Locate C4 by the global pointer AND the entity list: the global
    /// pointer alone may represent only the local player's equipped C4.
    /// A world marker requires a real ownerless C4 entity and valid position.
    fn scan_c4<M:Memory>(&mut self,p:&mut M,list:u64,base:u64,pawns:&BTreeMap<u64,Player>)
        ->(Option<u32>,Option<BombWorld>,bool,u32) {
        let mut candidates=Vec::new();
        let mut seen=HashSet::new();
        if let Some(offset)=self.schema.offset("dwWeaponC4") {
            let ptr=p.u64(base+offset);
            if ptr!=0 && seen.insert(ptr){candidates.push(ptr);}
        }
        let highest=self.schema.offset("dwGameEntitySystem_highestEntityIndex")
            .map(|offset|p.i32(list+offset))
            .filter(|idx|(64..=8191).contains(idx))
            .unwrap_or(2048).min(4095) as u32;
        let stride=self.layout.stride;
        for chunk in 0..=((highest as usize)>>9) {
            let block=self.block(p,list,(chunk*512) as u32);
            if block==0 {continue;}
            for begin in (0..512).step_by(128) {
                let first=chunk*512+begin;
                if first>highest as usize{break;}
                let count=(highest as usize+1-first).min(128);
                if let Some(raw)=p.read(block+begin as u64*stride as u64,count*stride) {
                    for i in 0..count {
                        let offset=i*stride;
                        let ptr=u64::from_le_bytes(raw[offset..offset+8].try_into().unwrap());
                        if ptr!=0 && ptr>=0x10000 && seen.insert(ptr){candidates.push(ptr);}
                    }
                }
            }
        }
        let scene=self.schema.field("C_BaseEntity","m_pGameSceneNode");
        let origin=self.schema.field("CGameSceneNode","m_vecAbsOrigin");
        let owner_off=self.schema.field("C_BaseEntity","m_hOwnerEntity");
        let mut carried:Option<u32>=None;
        let mut dropped:Option<BombWorld>=None;
        let mut ownerless=false;
        let mut checked=0;
        for ptr in candidates {
            if !self.item(p,ptr).is_some_and(|(id,_)|id==49){continue;}
            checked+=1;
            let raw_owner=owner_off.and_then(|o|p.read(ptr+o,4));
            let owner=match raw_owner {
                Some(raw)=>u32::from_le_bytes(raw.try_into().unwrap()),
                None=>continue, // Unknown owner is not the same as dropped C4.
            };
            if owner!=0 && owner!=u32::MAX && (owner & 0x7fff)!=0 {
                let pawn=self.entity(p,list,owner & 0x7fff);
                if let Some(player)=pawns.get(&pawn) {
                    if player.team==2 && player.life_state=="alive"{carried=Some(player.id);}
                }
                continue;
            }
            ownerless=true;
            if dropped.is_some(){continue;}
            let xyz=scene.and_then(|o|{let node=p.u64(ptr+o);
                if node!=0 {origin.and_then(|vo|p.vec3(node+vo))}else{None}})
                .filter(|v|*v!=[0.,0.,0.]);
            let last=self.schema.field("C_C4","m_vecLastValidDroppedPosition")
                .and_then(|o|p.vec3(ptr+o)).filter(|v|*v!=[0.,0.,0.]);
            if let Some(pos)=xyz.or(last) {
                dropped=Some(BombWorld{position:pos,status:"dropped".into(),source:"entity".into()});
            }
        }
        // A planted bomb is a separate entity; it must not be shown as "dropped".
        if let Some(offset)=self.schema.offset("dwPlantedC4"){
            let ptr=p.u64(base+offset);
            let active=self.schema.field("C_PlantedC4","m_bBombTicking")
                .and_then(|o|p.read(ptr+o,1))
                .and_then(|raw|raw.first().copied())
                .is_some_and(|b|b==1);
            if ptr!=0 && active {
                let pos=scene.and_then(|o|{let node=p.u64(ptr+o);
                    if node!=0 {origin.and_then(|vo|p.vec3(node+vo))}else{None}})
                    .filter(|v|*v!=[0.,0.,0.]);
                if let Some(position)=pos {
                    return (None,Some(BombWorld{position,status:"planted".into(),source:"planted_entity".into()}),true,checked);
                }
                return (None,None,true,checked);
            }
        }
        if carried.is_some(){(carried,None,false,checked)}
        else {(None,dropped,ownerless,checked)}
    }
    /// Source 2 designer-name classification, including short-lived throws.
    fn grenade_class(name:&str)->Option<&'static str>{
        let n=name.trim_start_matches("c_");
        if n=="inferno"{Some("inferno")}
        else if n.contains("smokegrenade_projectile"){Some("smoke")}
        else if n.contains("molotov_projectile")||n.contains("incendiarygrenade_projectile"){Some("molotov")}
        else if n.contains("flashbang_projectile"){Some("flash")}
        else if n.contains("hegrenade_projectile")||n.contains("fraggrenade_projectile"){Some("he")}
        else if n.contains("decoy_projectile"){Some("decoy")}
        else{None}
    }
    fn grenade_trail<M:Memory>(&self,p:&mut M,entity:u64)->Vec<[f32;3]>{
        let off=match self.schema.field("C_BaseCSGrenadeProjectile","m_arrTrajectoryTrailPoints"){Some(v)=>v,None=>return vec![]};
        let raw=match p.read(entity+off,16){Some(v)=>v,None=>return vec![]};
        let ptr=u64::from_le_bytes(raw[0..8].try_into().unwrap());
        let count=i32::from_le_bytes(raw[8..12].try_into().unwrap());
        if ptr<0x10000||count<=0||count>64{return vec![];}
        let points=match p.read(ptr,count as usize*12){Some(v)=>v,None=>return vec![]};
        points.chunks_exact(12).filter_map(|v|{
            let x=f32::from_le_bytes(v[0..4].try_into().ok()?);
            let y=f32::from_le_bytes(v[4..8].try_into().ok()?);
            let z=f32::from_le_bytes(v[8..12].try_into().ok()?);
            if [x,y,z].iter().all(|v|v.is_finite()&&v.abs()<100000.){Some([x,y,z])}else{None}
        }).collect()
    }
    fn scan_grenades<M:Memory>(&mut self,p:&mut M,list:u64,diag:&mut Diagnostics)->Vec<GrenadeView>{
        let ident_off=self.schema.field("CEntityInstance","m_pEntity").unwrap_or(0x10);
        // The identity type is from entity2.dll (not client.dll schema JSON).
        let scene_off=match self.schema.field("C_BaseEntity","m_pGameSceneNode"){Some(v)=>v,None=>return vec![]};
        let origin_off=match self.schema.field("CGameSceneNode","m_vecAbsOrigin"){Some(v)=>v,None=>return vec![]};
        let highest=self.schema.offset("dwGameEntitySystem_highestEntityIndex")
            .map(|off|p.i32(list+off))
            .filter(|n|(64..=8191).contains(n))
            .unwrap_or(2048).min(4095) as usize;
        let mut out=Vec::new();let stride=self.layout.stride;
        for chunk in 0..=(highest>>9){
            let block=self.block(p,list,(chunk*512) as u32);
            if block==0{continue;}
            for start in (0..512).step_by(128){
                let first=chunk*512+start;
                if first>highest{break;}
                let count=(highest+1-first).min(128);
                let raw=match p.read(block+start as u64*stride as u64,count*stride){Some(v)=>v,None=>continue};
                for i in 0..count{
                    if out.len()>=60{break;}
                    let off=i*stride;let ptr=u64::from_le_bytes(raw[off..off+8].try_into().unwrap());
                    if ptr<0x10000{continue;}
                    let ident=p.u64(ptr+ident_off);
                    if ident<0x10000{continue;}
                    let name_ptr=p.u64(ident+0x20);
                    if name_ptr<0x10000{continue;}
                    let name=p.cstr(name_ptr,64).to_ascii_lowercase();
                    let kind=match Self::grenade_class(&name){Some(n)=>n,None=>continue};
                    let node=p.u64(ptr+scene_off);
                    let pos=if node>=0x10000{p.vec3(node+origin_off)}else{None};
                    let pos=match pos{Some(v)=>v,None=>continue};
                    let index=(first+i) as u32;
                    if kind=="inferno"{
                        let count_off=match self.schema.field("C_Inferno","m_fireCount"){Some(v)=>v,None=>continue};
                        let fire_count=p.i32(ptr+count_off);
                        if !(1..=64).contains(&fire_count){continue;}
                        let off=match self.schema.field("C_Inferno","m_firePositions"){Some(v)=>v,None=>continue};
                        let data=match p.read(ptr+off,64*12){Some(v)=>v,None=>continue};
                        let mask=self.schema.field("C_Inferno","m_bFireIsBurning").and_then(|o|p.read(ptr+o,64));
                        let mut cells=Vec::new();
                        for j in 0..fire_count as usize{
                            if mask.as_ref().is_some_and(|m|m[j]==0){continue;}
                            let raw=&data[j*12..j*12+12];
                            let point=[f32::from_le_bytes(raw[0..4].try_into().unwrap()),
                                f32::from_le_bytes(raw[4..8].try_into().unwrap()),
                                f32::from_le_bytes(raw[8..12].try_into().unwrap())];
                            if point.iter().all(|v|v.is_finite()&&v.abs()<100000.)&&point!=[0.,0.,0.]{cells.push(point);}
                        }
                        if cells.is_empty(){continue;}
                        let n=cells.len() as f32;
                        let center=[cells.iter().map(|p|p[0]).sum::<f32>()/n,
                            cells.iter().map(|p|p[1]).sum::<f32>()/n,cells.iter().map(|p|p[2]).sum::<f32>()/n];
                        let radius=cells.iter().map(|p|((p[0]-center[0]).powi(2)+(p[1]-center[1]).powi(2)).sqrt()).fold(0f32,f32::max)+38.;
                        out.push(GrenadeView{id:index,kind:"molotov".into(),state:"active".into(),
                            position:center,radius_units:radius.clamp(45.,250.),trajectory:vec![],fire_cells:cells});
                    }else{
                        let active=kind=="smoke" && self.schema.field("C_SmokeGrenadeProjectile","m_bDidSmokeEffect")
                            .and_then(|o|p.read(ptr+o,1)).is_some_and(|v|v[0]==1);
                        let position=if active {
                            self.schema.field("C_SmokeGrenadeProjectile","m_vSmokeDetonationPos")
                                .and_then(|o|p.vec3(ptr+o)).filter(|v|*v!=[0.,0.,0.]).unwrap_or(pos)
                        }else{pos};
                        let mut points=if active{vec![]}else{self.grenade_trail(p,ptr)};
                        // Seed the first visible segment from the actual throw
                        // origin, even for short-lived HE and flash projectiles.
                        if !active&&points.is_empty(){
                            let initial=self.schema.field("C_BaseCSGrenadeProjectile","m_vInitialPosition")
                                .and_then(|o|p.vec3(ptr+o))
                                .filter(|v|v.iter().all(|x|x.is_finite()&&x.abs()<100000.)&&*v!=[0.,0.,0.]);
                            if let Some(from)=initial{
                                if (from[0]-pos[0]).hypot(from[1]-pos[1])>20.0{points.extend([from,pos]);}
                            }
                        }
                        out.push(GrenadeView{id:index,kind:kind.into(),state:if active{"active"}else{"flying"}.into(),
                            position,radius_units:if kind=="smoke"{160.}else{135.},trajectory:points,fire_cells:vec![]});
                    }
                }
            }
        }
        diag.set("grenades_found",json!(out.len()));
        out
    }

    /// ObserverService lives on the local observer pawn, not on the watched
    /// pawn/controller. 4=in-eye, 5=chase. Other modes must not pin the map.
    fn read_observer_view<M:Memory>(&mut self,p:&mut M,list:u64,local:u64,
        local_pawn:u64,pawns:&BTreeMap<u64,Player>)->Option<ObserverView>{
        if local==0{return None;}
        let alive=self.schema.field("CCSPlayerController","m_bPawnIsAlive")
            .and_then(|off|try_u8(p,local+off));
        if alive==Some(1){return None;}
        let handle=self.schema.field("CCSPlayerController","m_hObserverPawn")
            .map(|off|p.u32(local+off)).filter(|h|*h!=0 && *h!=u32::MAX && (h&0x7fff)!=0);
        let observer=handle.map(|h|self.entity(p,list,h&0x7fff)).filter(|v|*v!=0);
        let service_off=self.schema.field("C_BasePlayerPawn","m_pObserverServices")?;
        let mode_off=self.schema.field("CPlayer_ObserverServices","m_iObserverMode")?;
        let target_off=self.schema.field("CPlayer_ObserverServices","m_hObserverTarget")?;
        let scene_off=self.schema.field("C_BaseEntity","m_pGameSceneNode")?;
        let pos_off=self.schema.field("CGameSceneNode","m_vecAbsOrigin")?;
        let health_off=self.schema.field("C_BaseEntity","m_iHealth");
        let team_off=self.schema.field("C_BaseEntity","m_iTeamNum");
        let yaw_off=self.schema.field("C_CSPlayerPawn","m_angEyeAngles");
        // CS2 can put services on an observer pawn or retain them on the
        // previous local player pawn during spectator transitions.
        for entity in [observer,Some(local_pawn).filter(|v|*v!=0)].into_iter().flatten(){
            let services=p.u64(entity+service_off);
            if services==0{continue;}
            let mode=match try_u8(p,services+mode_off){Some(4|5)=>p.u8(services+mode_off),_=>continue};
            let handle=p.u32(services+target_off);
            if handle==0 || handle==u32::MAX || handle&0x7fff==0 {continue;}
            let index=handle&0x7fff;
            let pawn=self.entity(p,list,index);
            if pawn==0 || pawn==entity || pawn==local_pawn {continue;}
            // Prefer the controller-derived player state already validated
            // by the radar reader, fall back to validated entity properties.
            if let Some(player)=pawns.get(&pawn) {
                if player.life_state=="alive"{
                    return Some(ObserverView{target_entity_id:index,
                        player_id:Some(player.id),name:player.name.clone(),
                        position:player.position,yaw:player.yaw,mode});
                }
                continue;
            }
            let hp=health_off.and_then(|off|try_i32(p,pawn+off));
            if !hp.is_some_and(|v|(1..=200).contains(&v)){continue;}
            let team=team_off.and_then(|off|try_u8(p,pawn+off));
            if !matches!(team,Some(2|3)){continue;}
            let node=p.u64(pawn+scene_off);
            if node==0{continue;}
            let position=match p.vec3(node+pos_off){
                Some(v) if v!=[0.,0.,0.] && v.iter().all(|x|x.is_finite())=>v,
                _=>continue
            };
            let yaw=yaw_off.and_then(|off|p.vec3(pawn+off))
                .and_then(|angles|normalized_yaw(Some(angles)));
            return Some(ObserverView{target_entity_id:index,player_id:None,
                name:format!("Игрок #{index}"),position,yaw,mode});
        }
        None
    }
    pub fn current_map<M:Memory>(&self,p:&mut M,base:u64)->Option<String>{
        let off=self.schema.offset("dwGlobalVars")?;let ptr=p.u64(base+off);if ptr==0{return None;}
        let strptr=p.u64(ptr+0x180);if strptr==0{return None;}
        let map=p.cstr(strptr,96);normalize_map_name(&map)
    }
    pub fn snapshot<M:Memory>(&mut self,p:&mut M,base:u64,engine:u64,pid:u32,details:bool,grenade_scan:bool)->Result<Snapshot,String>{
        let start=Instant::now();self.chunk_cache.clear();
        let before_read=p.read_count();let before_fail=p.fail_count();
        let mut diag=Diagnostics::default();
        let list_off=self.schema.offset("dwEntityList").ok_or("Missing dwEntityList")?;
        let list=p.u64(base+list_off);if list==0{return Err("dwEntityList empty; loading scene or invalid offsets".into());}
        let local=p.u64(base+self.schema.offset("dwLocalPlayerController").ok_or("Missing local controller offset")?);
        let local_pawn=opt_u64(p,base,self.schema.offset("dwLocalPlayerPawn"));
        let bomb_timer=self.planted_c4_timer(p,base);
        diag.set("bomb_timer_available",json!(bomb_timer.is_some()));
        diag.set("local_controller_found",json!(if local!=0 {1}else{0}));
        diag.set("local_pawn_found",json!(if local_pawn!=0 {1}else{0}));
        self.probe_layout(p,list,base+list_off,local);self.chunk_cache.clear();
        let list=self.layout.root;
        diag.set("layout_confirmed",json!(self.layout.confirmed));diag.set("layout_local_slot",json!(self.layout.slot));
        diag.set("layout_stride",json!(self.layout.stride));diag.set("layout_chunk_offset",json!(self.layout.chunk_offset));
        diag.set("layout_candidates_checked",json!(self.probes));
        let s=&self.schema;
        let team_off=field(s,"C_BaseEntity","m_iTeamNum")?;
        let health_off=field(s,"C_BaseEntity","m_iHealth")?;
        let handle_off=field(s,"CCSPlayerController","m_hPlayerPawn")?;
        let base_handle=s.field("CBasePlayerController","m_hPawn");
        let pending_off=s.field("CCSPlayerController","m_iPendingTeamNum");
        let ctl_health=s.field("CCSPlayerController","m_iPawnHealth");
        let alive_off=s.field("CCSPlayerController","m_bPawnIsAlive");
        let old_origin=s.field("C_BasePlayerPawn","m_vOldOrigin");
        let scene_off=field(s,"C_BaseEntity","m_pGameSceneNode")?;
        let abs_off=field(s,"CGameSceneNode","m_vecAbsOrigin")?;
        let name_off=s.field("CBasePlayerController","m_iszPlayerName");
        let reverse_off=s.field("C_BasePlayerPawn","m_hController");
        let yaw_off=s.field("C_CSPlayerPawn","m_angEyeAngles");
        let money_off=s.field("CCSPlayerController","m_pInGameMoneyServices");
        let account_off=s.field("CCSPlayerController_InGameMoneyServices","m_iAccount");
        let armor_off=s.field("C_CSPlayerPawn","m_ArmorValue");
        let helmet_off=s.field("CCSPlayerController","m_bPawnHasHelmet");
        let defuser_off=s.field("CCSPlayerController","m_bPawnHasDefuser");
        let mut local_team=if local!=0 {p.u8(local+team_off)} else {0};
        let (controllers,fallback)=self.controllers(p,list,local);
        diag.set("controller_entries",json!(controllers.len()));
        diag.set("local_controller_in_slots",json!(if local!=0 && !fallback {1}else{0}));
        diag.set("local_fallback_used",json!(if fallback {1}else{0}));
        let mut local_motion:Option<LocalMotion>=None;
        let mut seen=HashSet::new();
        let mut pawns:BTreeMap<u64,Player>=BTreeMap::new();let mut histogram:HashMap<String,u32>=HashMap::new();
        for (slot,ctl) in controllers {
            if !seen.insert(ctl){continue;}
            let is_local=local!=0 && local==ctl;
            let mut team=p.u8(ctl+team_off);*histogram.entry(team.to_string()).or_default()+=1;
            let h=p.u32(ctl+handle_off);let index=h&0x7fff;
            if h!=u32::MAX&&index>0{diag.add("pawn_handles");diag.add("controllers_with_handle");}else{diag.add("invalid_handles");}
            let mut pawn=if h!=u32::MAX&&index>0 {self.entity(p,list,index)}else{0};
            if pawn==0 {if let Some(o)=base_handle {
                let alt=p.u32(ctl+o);
                if alt!=u32::MAX&&(alt&0x7fff)>0&&(alt&0x7fff)!=index{
                    pawn=self.entity(p,list,alt&0x7fff);if pawn!=0{diag.add("alternate_pawn_used");}
                }
            }}
            if pawn==0&&is_local&&local_pawn!=0{pawn=local_pawn;diag.set("local_fallback_used",json!(1));}
            if pawn==0{diag.add("missing_pawn");continue;}diag.add("resolved_pawns");
            if let Some(o)=reverse_off{if slot>0 {
                let reverse=p.u32(pawn+o);
                if reverse!=0&&reverse!=u32::MAX{
                    diag.add("reverse_handle_checks");if reverse&0x7fff!=slot{diag.add("reverse_handle_mismatches");continue;}
                }
            }}
            let pawn_team=p.u8(pawn+team_off);
            if team!=2&&team!=3&&[2,3].contains(&pawn_team){team=pawn_team;diag.add("pawn_team_used");}
            if team!=2&&team!=3{if let Some(o)=pending_off {
                let pending=p.i32(ctl+o);if pending==2||pending==3{team=pending as u8;diag.add("pending_team_used");}
            }}
            if team!=2&&team!=3{diag.add("unknown_team");continue;}diag.add("valid_teams");
            if is_local{local_team=team;}
            let mut hp=try_i32(p,pawn+health_off);
            let alive=alive_off.and_then(|o|try_u8(p,ctl+o)).and_then(|v|if v<=1 {Some(v==1)}else{None});
            if !matches!(hp,Some(1..=200)) && alive==Some(true) {
                if let Some(v)=opt_i32(p,ctl,ctl_health){if (1..=200).contains(&v){hp=Some(v);diag.add("controller_health_used");}}
            }
            let life=life_state(hp,alive);
            match life {"alive"=>diag.add("alive_pawns"),"dead"=>diag.add("dead_players"),_=>diag.add("unknown_life_players")};
            if life!="alive"{diag.add("missing_health");}
            let node=p.u64(pawn+scene_off);if node!=0{diag.add("scene_nodes");}
            let mut xyz=if node!=0 {p.vec3(node+abs_off)}else{None};
            if xyz.map_or(true,|v|v==[0.,0.,0.]){xyz=old_origin.and_then(|o|p.vec3(pawn+o));}
            let xyz=match xyz{Some(v) if v!=[0.,0.,0.]=>v,_=>{diag.add("missing_position");continue;}};
            diag.add("position_reads");if life!="alive"{diag.add("unverified_health_positions");}
            let angles=if life=="alive"{yaw_off.and_then(|o|p.vec3(pawn+o))}else{None};
            let yaw=normalized_yaw(angles);if angles.is_some()&&yaw.is_none(){diag.add("bad_angles");}
            if is_local && life=="alive" {
                let vel=self.schema.field("C_BaseEntity","m_vecAbsVelocity").and_then(|o|p.vec3(pawn+o));
                let speed=vel.map(|v|v[0].hypot(v[1])).filter(|v|v.is_finite() && *v<2000.);
                let flags=self.schema.field("C_BaseEntity","m_fFlags").and_then(|o|p.read(pawn+o,4))
                    .and_then(|v|v.try_into().ok()).map(u32::from_le_bytes);
                let duck=self.schema.field("C_CSPlayerPawn","m_flDuckAmount").and_then(|o|p.read(pawn+o,4))
                    .and_then(|v|v.try_into().ok()).map(f32::from_le_bytes)
                    .filter(|v|v.is_finite() && (0.0..=1.1).contains(v));
                let shots=self.schema.field("C_CSPlayerPawn","m_iShotsFired").and_then(|o|try_i32(p,pawn+o))
                    .filter(|v|(0..=99).contains(v));
                local_motion=Some(LocalMotion{speed,on_ground:flags.map(|f|f&1!=0),
                    ducking:duck.map(|v|v>0.6),shots_fired:shots,weapon:None});
            }
            let name=name_off.map(|o|p.cstr(ctl+o,128)).unwrap_or_default();
            let (mut weapon,mut inventory,mut has_c4)=("—".to_owned(),vec![],None);
            let (mut money,mut armor,mut helmet,mut defuser)=(None,0,false,false);
            if details {
                (weapon,inventory,has_c4)=self.player_items(p,list,pawn);
                let money_ptr=opt_u64(p,ctl,money_off);money=opt_i32(p,money_ptr,account_off).filter(|n|(0..=100000).contains(n));
                armor=opt_i32(p,pawn,armor_off).unwrap_or(0).clamp(0,200);
                helmet=helmet_off.is_some_and(|o|p.u8(ctl+o)!=0);
                defuser=defuser_off.is_some_and(|o|p.u8(ctl+o)!=0);
            }
            if is_local {if let Some(m)=local_motion.as_mut(){if weapon!="—"{m.weapon=Some(weapon.clone());}}}
            let entry=Player{id:slot,name:if name.is_empty(){if is_local{"Вы".into()}else{format!("Player {slot}")}}else{name},
                team,local:is_local,health:hp.filter(|n|(0..=200).contains(n)),life_state:life.into(),health_verified:life=="alive",
                armor,helmet,defuser,money,position:xyz,yaw,weapon,inventory,has_c4,c4_source:None};
            if let Some(prev)=pawns.get(&pawn){
                diag.add("duplicate_pawns");
                let rank=|p:&Player|(p.local as u8,match p.life_state.as_str(){"alive"=>2,"dead"=>1,_=>0});
                if rank(&entry)>rank(prev){pawns.insert(pawn,entry);}
            }else{pawns.insert(pawn,entry);}
        }
        let mut global_owner=None;let mut unowned=false;let mut bomb_world=None;
        if details {
            let (owner,world,ownerless,scanned)=self.scan_c4(p,list,base,&pawns);
            global_owner=owner;bomb_world=world;unowned=ownerless;
            diag.set("c4_entities_scanned",json!(scanned));
            diag.set("c4_world_found",json!(bomb_world.is_some()));
            if let Some(id)=owner {
                if let Some(player)=pawns.values_mut().find(|player|player.id==id) {
                    player.has_c4=Some(true);player.c4_source=Some("entity_owner".into());
                }
            }
        }
        let observer_view=self.read_observer_view(p,list,local,local_pawn,&pawns);
        diag.set("observer_target_found",json!(observer_view.is_some()));
        diag.set("observer_mode",json!(observer_view.as_ref().map(|v|v.mode)));
        let grenades=if grenade_scan{self.scan_grenades(p,list,&mut diag)}else{vec![]};
        let mut players:Vec<Player>=pawns.into_values().collect();players.sort_by_key(|p|p.id);
        diag.set("c4_global_owner_found",json!(if global_owner.is_some(){1}else{0}));
        diag.set("c4_global_unowned",json!(if unowned {1}else{0}));
        // Debug addresses are never returned in diagnostics.
        let probe=if local_pawn!=0{local_pawn}else if local!=0{
            let h=p.u32(local+handle_off);if h>0&&h!=u32::MAX{self.entity(p,list,h&0x7fff)}else{0}
        }else{0};
        let (hp_valid,node_ok,pos_ok)=if probe>0{
            let h=p.i32(probe+health_off);let n=p.u64(probe+scene_off);
            let v=if n>0{p.vec3(n+abs_off)}else{None};
            ((1..=200).contains(&h),n!=0,v.is_some_and(|v|v!=[0.,0.,0.]))
        }else{(false,false,false)};
        diag.set("local_pawn_probe",json!({"pointer":probe>0,"health_valid":hp_valid,"scene_node":node_ok,"position_valid":pos_ok}));
        diag.set("players",json!(players.len()));
        diag.set("read_attempts",json!(p.read_count()-before_read));diag.set("read_failures",json!(p.fail_count()-before_fail));
        diag.set("last_win32_error",json!(p.last_error()));diag.set("offsets_build",self.schema.info["build_number"].clone());
        diag.set("offsets_timestamp",self.schema.info["timestamp"].clone());
        let build=if engine!=0{self.schema.engine_offset("dwBuildNumber").map(|o|p.u32(engine+o))}else{None};
        diag.set("game_build",json!(build));
        let same=build.and_then(|b|self.schema.info["build_number"].as_u64().map(|v|v==b as u64));
        diag.set("offsets_match_game",json!(same));
        diag.set("read_ms",json!((start.elapsed().as_secs_f64()*100000.).round()/100.));
        diag.set("controller_team_histogram",json!(histogram));
        let verified=players.iter().filter(|p|p.life_state!="unknown").count();
        let status=if players.is_empty(){"no_players"}else if verified==players.len(){"live"}else{"partial"};
        let message=if players.is_empty(){
            let reason=if diag.num("controller_entries")==0{"Список контроллеров пуст."}
            else if diag.num("resolved_pawns")==0{"Контроллеры найдены, но pawn не разрешаются."}
            else if diag.num("valid_teams")==0{"Не удалось подтвердить команды игроков."}
            else if !self.layout.confirmed && local!=0{"Локальный контроллер не найден в слотах списка сущностей."}
            else{"Координаты pawn не читаются: проверьте schema/игровой процесс."};Some(reason.to_owned())
        }else if verified!=players.len(){Some("Часть игроков имеет неподтверждённое состояние жизни".into())}else{None};
        Ok(Snapshot{status:status.into(),pid,local_team,
            bomb_carrier_id:if bomb_world.is_some(){None}else{global_owner.or_else(||unique_bomb_carrier(&players))},
            bomb_world,bomb_timer,c4_global_owner_id:global_owner,c4_unowned:unowned,players,grenades,grenades_read:grenade_scan,observer_view,details_read:details,
            local_motion,diagnostics:diag.value(),time:now_secs(),message,..Snapshot::status(status,"")})
    }
}
#[cfg(test)]mod projectile_name_tests {
 use super::CS2Reader;
 #[test]fn all_grenade_projectiles(){
  for (name,kind) in [
   ("smokegrenade_projectile","smoke"),("molotov_projectile","molotov"),
   ("incendiarygrenade_projectile","molotov"),("flashbang_projectile","flash"),
   ("hegrenade_projectile","he"),("decoy_projectile","decoy"),
   ("inferno","inferno"),("c_hegrenade_projectile","he")
  ]{
   assert_eq!(CS2Reader::grenade_class(name),Some(kind));
  }
  assert_eq!(CS2Reader::grenade_class("weapon_flashbang"),None);
 }
}
