//! Source 2 read-only, best-effort controller/pawn reader.
use crate::model::{Diagnostics, Player, Snapshot, life_state, normalized_yaw, unique_bomb_carrier, now_secs, normalize_map_name};
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
    fn global_c4_owner<M:Memory>(&mut self,p:&mut M,list:u64,base:u64)->(Option<u64>,bool){
        let off=match self.schema.offset("dwWeaponC4"){Some(o)=>o,None=>return(None,false)};
        let raw=match p.read(base+off,8){Some(r)=>r,None=>return(None,false)};
        let ptr=u64::from_le_bytes(raw.try_into().unwrap());
        if ptr==0 || !self.item(p,ptr).is_some_and(|(id,_)|id==49){return(None,false);}
        let owner_off=match self.schema.field("C_BaseEntity","m_hOwnerEntity"){Some(o)=>o,None=>return(None,false)};
        let raw=match p.read(ptr+owner_off,4){Some(r)=>r,None=>return(None,false)};
        let owner=u32::from_le_bytes(raw.try_into().unwrap());
        if owner==0||owner==u32::MAX{return(None,true);}
        let pawn=self.entity(p,list,owner&0x7fff);
        (if pawn==0{None}else{Some(pawn)},false)
    }
    pub fn current_map<M:Memory>(&self,p:&mut M,base:u64)->Option<String>{
        let off=self.schema.offset("dwGlobalVars")?;let ptr=p.u64(base+off);if ptr==0{return None;}
        let strptr=p.u64(ptr+0x180);if strptr==0{return None;}
        let map=p.cstr(strptr,96);normalize_map_name(&map)
    }
    pub fn snapshot<M:Memory>(&mut self,p:&mut M,base:u64,engine:u64,pid:u32,details:bool)->Result<Snapshot,String>{
        let start=Instant::now();self.chunk_cache.clear();
        let before_read=p.read_count();let before_fail=p.fail_count();
        let mut diag=Diagnostics::default();
        let list_off=self.schema.offset("dwEntityList").ok_or("Missing dwEntityList")?;
        let list=p.u64(base+list_off);if list==0{return Err("dwEntityList empty; loading scene or invalid offsets".into());}
        let local=p.u64(base+self.schema.offset("dwLocalPlayerController").ok_or("Missing local controller offset")?);
        let local_pawn=opt_u64(p,base,self.schema.offset("dwLocalPlayerPawn"));
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
            let entry=Player{id:slot,name:if name.is_empty(){if is_local{"Вы".into()}else{format!("Player {slot}")}}else{name},
                team,local:is_local,health:hp.filter(|n|(0..=200).contains(n)),life_state:life.into(),health_verified:life=="alive",
                armor,helmet,defuser,money,position:xyz,yaw,weapon,inventory,has_c4,c4_source:None};
            if let Some(prev)=pawns.get(&pawn){
                diag.add("duplicate_pawns");
                let rank=|p:&Player|(p.local as u8,match p.life_state.as_str(){"alive"=>2,"dead"=>1,_=>0});
                if rank(&entry)>rank(prev){pawns.insert(pawn,entry);}
            }else{pawns.insert(pawn,entry);}
        }
        let mut global_owner=None;let mut unowned=false;
        if details {
            let (owner,not_owned)=self.global_c4_owner(p,list,base);
            unowned=not_owned;
            if let Some(owner)=owner {if let Some(player)=pawns.get_mut(&owner){
                if player.team==2&&player.life_state=="alive"{global_owner=Some(player.id);player.has_c4=Some(true);player.c4_source=Some("global".into());}
            }}
        }
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
        Ok(Snapshot{status:status.into(),pid,local_team,bomb_carrier_id:global_owner.or_else(||unique_bomb_carrier(&players)),
            c4_global_owner_id:global_owner,c4_unowned:unowned,players,details_read:details,
            diagnostics:diag.value(),time:now_secs(),message,..Snapshot::status(status,"")})
    }
}