//! Read-only recovery of player globals from validated PE64 executable sections.
//! Signatures mirror a2x/cs2-dumper; recovered globals must resolve a real
//! local controller in the entity table before either offset is accepted.
use crate::process::Memory;
use std::collections::BTreeSet;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct RecoveredGlobals {pub entity_list_offset:u64,pub local_controller_offset:u64}
#[derive(Clone,Copy)]
struct Section {start:u64,end:u64,flags:u32}
const EXEC:u32=0x2000_0000;
const READ:u32=0x4000_0000;
const CHUNK:usize=16*1024;
fn canonical(ptr:u64)->bool{(0x10000..=0x7fff_ffff_ffff).contains(&ptr)}
fn u16_at(bytes:&[u8],at:usize)->Option<u16>{Some(u16::from_le_bytes(bytes.get(at..at+2)?.try_into().ok()?))}
fn u32_at(bytes:&[u8],at:usize)->Option<u32>{Some(u32::from_le_bytes(bytes.get(at..at+4)?.try_into().ok()?))}
fn pointer<M:Memory>(p:&mut M,addr:u64)->Option<u64>{
    let bytes=p.read(addr,8)?;Some(u64::from_le_bytes(bytes.as_slice().try_into().ok()?))
}
fn sections<M:Memory>(p:&mut M,base:u64)->Option<(u64,Vec<Section>)>{
    if !canonical(base){return None;}
    let dos=p.read(base,64)?;
    if dos.get(..2)!=Some(b"MZ"){return None;}
    let nt=u32_at(&dos,60)? as u64;
    if !(64..=1024*1024).contains(&nt){return None;}
    let header=p.read(base.checked_add(nt)?,24)?;
    if header.get(..4)!=Some(b"PE\0\0")||u16_at(&header,4)?!=0x8664{return None;}
    let count=u16_at(&header,6)? as usize;let optional=u16_at(&header,20)? as u64;
    if !(1..=96).contains(&count)||!(112..=4096).contains(&optional){return None;}
    let opts=p.read(base.checked_add(nt)?.checked_add(24)?,112)?;
    if u16_at(&opts,0)?!=0x20b{return None;}
    let image=u32_at(&opts,56)? as u64;let headers=u32_at(&opts,60)? as u64;
    let table=nt.checked_add(24)?.checked_add(optional)?;
    if !(4096..=256*1024*1024).contains(&image)||!canonical(base.checked_add(image-1)?)
        || headers>image||table.checked_add(count as u64*40)?>headers{return None;}
    let raw=p.read(base.checked_add(table)?,count*40)?;let mut sections=Vec::new();
    for section in raw.chunks_exact(40){
        let start=u32_at(section,12)? as u64;
        let len=u32_at(section,8)?.max(u32_at(section,16)?) as u64;
        if len==0{continue;}
        let end=start.checked_add(len)?;
        if start<headers||end>image{return None;}
        sections.push(Section{start,end,flags:u32_at(section,36)?});
    }
    sections.sort_by_key(|s|s.start);
    if sections.windows(2).any(|s|s[0].end>s[1].start){return None;}
    Some((image,sections))
}
fn collect(bytes:&[u8],rva:u64,matches:&mut [BTreeSet<u64>;2]){
    for i in 0..bytes.len(){
        let rest=&bytes[i..];
        if rest.len()>=13&&rest[..3]==[0x48,0x89,0x0d]&&rest[7]==0xe9&&rest[12]==0xcc{matches[0].insert(rva+i as u64);}
        if rest.len()>=10&&rest[..3]==[0x48,0x8b,0x05]&&rest[7..10]==[0x41,0x89,0xbe]{matches[1].insert(rva+i as u64);}
        if matches.iter().any(|m|m.len()>1){return;}
    }
}
fn scan<M:Memory>(p:&mut M,base:u64,section:Section,matches:&mut [BTreeSet<u64>;2]){
    let mut at=section.start;let mut tail=Vec::new();
    while at<section.end{
        let len=(section.end-at).min(CHUNK as u64) as usize;
        let big=p.read(base+at,len);
        let step=if big.is_some(){len}else{4096};
        let mut inside=0;
        while inside<len{
            let n=(len-inside).min(step);
            let bytes=if inside==0&&big.is_some(){big.clone()}else{p.read(base+at+inside as u64,n)};
            if let Some(bytes)=bytes{
                let mut joined=tail.clone();joined.extend_from_slice(&bytes);
                collect(&joined,at+inside as u64-tail.len() as u64,matches);
                if matches.iter().any(|m|m.len()>1){return;}
                tail=joined[joined.len().saturating_sub(16)..].to_vec();
            }else{tail.clear();}
            inside+=n;
        }
        at+=len as u64;
    }
}
fn global_target<M:Memory>(p:&mut M,base:u64,signature:u64,sections:&[Section])->Option<u64>{
    let bytes=p.read(base.checked_add(signature)?,7)?;
    let displacement=i32::from_le_bytes(bytes.get(3..7)?.try_into().ok()?) as i64;
    let target=(signature as i64).checked_add(7)?.checked_add(displacement)?;
    let target=u64::try_from(target).ok()?;
    if !sections.iter().any(|s|s.flags&READ!=0&&s.flags&EXEC==0&&target>=s.start&&target.checked_add(8).is_some_and(|end|end<=s.end)){return None;}
    Some(target)
}
fn controller_in_list<M:Memory>(p:&mut M,list:u64,controller:u64)->bool{
    for offset in [0x10,0,8,0x18,0x20]{
        let Some(block)=pointer(p,list+offset).filter(|v|canonical(*v)) else{continue;};
        for stride in [0x78,0x70]{
            // Individual bounded reads also work across partially unreadable pages.
            for slot in 1..=64{
                if pointer(p,block+stride*slot)==Some(controller){return true;}
            }
        }
    }
    false
}
pub fn recover_player_globals<M:Memory>(p:&mut M,base:u64)->Option<RecoveredGlobals>{
    let (_,sections)=sections(p,base)?;let mut matches=[BTreeSet::new(),BTreeSet::new()];
    for section in sections.iter().filter(|s|s.flags&EXEC!=0){
        scan(p,base,*section,&mut matches);
        if matches.iter().any(|m|m.len()>1){return None;}
    }
    if matches.iter().any(|m|m.len()!=1){return None;}
    let entity_list_offset=global_target(p,base,*matches[0].first()?,&sections)?;
    let local_controller_offset=global_target(p,base,*matches[1].first()?,&sections)?;
    if entity_list_offset==local_controller_offset{return None;}
    let list=pointer(p,base+entity_list_offset).filter(|v|canonical(*v))?;
    let controller=pointer(p,base+local_controller_offset).filter(|v|canonical(*v))?;
    if !controller_in_list(p,list,controller){return None;}
    Some(RecoveredGlobals{entity_list_offset,local_controller_offset})
}

#[cfg(test)]mod tests{
    use super::*;use crate::process::FakeMemory;
    const BASE:u64=0x100000;
    fn put32(p:&mut FakeMemory,addr:u64,n:u32){p.put(addr,&n.to_le_bytes());}
    fn signature(p:&mut FakeMemory,at:u64,target:u64,entity:bool){
        let mut bytes=if entity{vec![0x48,0x89,0x0d,0,0,0,0,0xe9,0,0,0,0,0xcc]}else{vec![0x48,0x8b,0x05,0,0,0,0,0x41,0x89,0xbe]};
        bytes[3..7].copy_from_slice(&((target as i64-at as i64-7)as i32).to_le_bytes());p.put(BASE+at,&bytes);
    }
    fn fixture()->FakeMemory{
        let mut p=FakeMemory::new();p.put(BASE,&vec![0;0x9000]);p.put(BASE,b"MZ");put32(&mut p,BASE+60,0x80);
        p.put(BASE+0x80,b"PE\0\0");p.put(BASE+0x84,&0x8664u16.to_le_bytes());p.put(BASE+0x86,&2u16.to_le_bytes());
        p.put(BASE+0x94,&0xf0u16.to_le_bytes());p.put(BASE+0x98,&0x20bu16.to_le_bytes());
        put32(&mut p,BASE+0x98+56,0x9000);put32(&mut p,BASE+0x98+60,0x1000);
        // Data precedes text, so RIP displacements are negative.
        for (i,(start,len,flags)) in [(0x1000,0x1000,READ),(0x3000,0x6000,READ|EXEC)].into_iter().enumerate(){
            let section=BASE+0x188+i as u64*40;put32(&mut p,section+8,len);put32(&mut p,section+12,start);put32(&mut p,section+36,flags);
        }
        signature(&mut p,0x3100,0x1100,true);signature(&mut p,0x3200,0x1110,false);
        p.put(BASE+0x1100,&0x200000u64.to_le_bytes());p.put(BASE+0x1110,&0x400000u64.to_le_bytes());
        p.put(0x200010,&0x300000u64.to_le_bytes());p.put(0x300000+3*0x78,&0x400000u64.to_le_bytes());p
    }
    #[test]fn recovers_signed_rip_displacements_and_validates_controller(){
        assert_eq!(recover_player_globals(&mut fixture(),BASE),Some(RecoveredGlobals{entity_list_offset:0x1100,local_controller_offset:0x1110}));
    }
    #[test]fn finds_signature_crossing_a_chunk_boundary_without_duplicate_matches(){
        let mut p=fixture();p.put(BASE+0x3100,&[0;13]);signature(&mut p,0x3000+CHUNK as u64-6,0x1100,true);
        assert!(recover_player_globals(&mut p,BASE).is_some());
    }
    #[test]fn unreadable_page_does_not_hide_later_readable_signatures(){
        let mut p=fixture();p.put(BASE+0x3200,&[0;10]);signature(&mut p,0x6000,0x1110,false);
        for at in BASE+0x4000..BASE+0x5000{p.mem.remove(&at);}
        assert!(recover_player_globals(&mut p,BASE).is_some());
    }
    #[test]fn rejects_ambiguous_or_absent_signatures(){
        let mut p=fixture();signature(&mut p,0x3300,0x1100,true);assert!(recover_player_globals(&mut p,BASE).is_none());
        let mut p=fixture();p.put(BASE+0x3200,&[0;10]);assert!(recover_player_globals(&mut p,BASE).is_none());
    }
    #[test]fn stale_offsets_recover_into_a_populated_live_snapshot(){
        use crate::{game::CS2Reader,schema::Schema};use serde_json::json;
        let mut p=fixture();
        let offsets=json!({"client.dll":{"dwEntityList":0x200,"dwLocalPlayerController":0x208}});
        let classes=json!({"client.dll":{"classes":{
            "CCSPlayerController":{"fields":{"m_hPlayerPawn":0x20,"m_bPawnIsAlive":0x24}},
            "C_BaseEntity":{"fields":{"m_iTeamNum":0x10,"m_iHealth":0x14,"m_pGameSceneNode":0x28}},
            "CGameSceneNode":{"fields":{"m_vecAbsOrigin":0x10}}
        }}});
        // Initially both obsolete globals are readable zero. Recovery finds
        // the signed RIP-relative globals and validates controller slot 3.
        p.put(0x400000+0x10,&[2]);put32(&mut p,0x400000+0x20,100);p.put(0x400000+0x24,&[1]);
        p.put(0x300000+100*0x78,&0x500000u64.to_le_bytes());
        p.put(0x500000+0x10,&[2]);put32(&mut p,0x500000+0x14,100);
        p.put(0x500000+0x28,&0x600000u64.to_le_bytes());
        let position:Vec<u8>=[100f32,200.,64.].iter().flat_map(|v|v.to_le_bytes()).collect();p.put(0x600010,&position);
        let mut reader=CS2Reader::new(Schema::from_json(&offsets,&classes,json!({"build_number":123})).unwrap());
        for frame in 0..2{
            let snapshot=reader.snapshot(&mut p,BASE,0,42,false,false).unwrap();
            assert_eq!(snapshot.status,"live");assert_eq!(snapshot.players.len(),1);
            assert_eq!(snapshot.players[0].id,3);assert_eq!(snapshot.players[0].health,Some(100));
            assert_eq!(snapshot.players[0].position,[100.,200.,64.]);
            assert_eq!(snapshot.diagnostics["runtime_offsets_verified"],true);
            if frame==0{assert_eq!(snapshot.diagnostics["runtime_recovery_attempted"],true);}
            else{assert!(snapshot.diagnostics.get("runtime_recovery_attempted").is_none());}
        }
    }
    #[test]fn rejects_bad_pe_outside_targets_or_unvalidated_pointers(){
        let mut p=fixture();put32(&mut p,BASE+0x98+56,0x40000000);assert!(recover_player_globals(&mut p,BASE).is_none());
        let mut p=fixture();signature(&mut p,0x3100,0xa000,true);assert!(recover_player_globals(&mut p,BASE).is_none());
        let mut p=fixture();signature(&mut p,0x3100,0x3500,true);assert!(recover_player_globals(&mut p,BASE).is_none());
        let mut p=fixture();p.put(0x300000+3*0x78,&0u64.to_le_bytes());assert!(recover_player_globals(&mut p,BASE).is_none());
    }
}
