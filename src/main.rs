use cas_external_radar::{assets,game::CS2Reader,gsi,model::{Snapshot,demo_snapshot},process::WinProcess,server::HttpServer,schema::Schema,state::SharedState,tracker::BombTracker,grenade::GrenadeTracker,noise::NoiseTracker,overlay::OverlayController};
use serde_json::{json,Value};
use std::{collections::HashMap,env,fs,path::{Path,PathBuf},sync::{Arc,Mutex},thread,time::{Duration,Instant}};
use rand::{rngs::OsRng,RngCore};

#[derive(Default)]struct Args {demo:bool,lan:bool,offline:bool,refresh:bool,no_browser:bool,no_gsi_install:bool,
    port:u16,cfg:Option<PathBuf>}
fn parse_args()->Result<Args,String>{
    let mut a=Args{port:8765,..Args::default()};let mut args=env::args().skip(1);
    while let Some(arg)=args.next(){match arg.as_str(){
        "--demo"=>a.demo=true,"--lan"=>a.lan=true,"--offline"=>a.offline=true,"--refresh"=>a.refresh=true,
        "--no-browser"=>a.no_browser=true,"--no-gsi-install"=>a.no_gsi_install=true,
        "--port"=>a.port=args.next().ok_or("--port requires value")?.parse().map_err(|_|"bad port")?,
        "--cs2-cfg"=>a.cfg=Some(PathBuf::from(args.next().ok_or("--cs2-cfg requires directory")?)),
        "--help"|"-h"=>{println!("CAS External Radar Rust\n--demo --lan --port 8765 --offline --refresh --no-browser --no-gsi-install --cs2-cfg PATH");std::process::exit(0);}
        _=>return Err(format!("Unknown option {arg}")),
    }}Ok(a)
}
fn random_token(bytes:usize)->String{
    let mut buf=vec![0u8;bytes];OsRng.fill_bytes(&mut buf);
    // URL-safe hex, cryptographic RNG, no punctuation in HTML query.
    buf.iter().map(|x|format!("{x:02x}")).collect()
}
fn exe_root()->PathBuf{
    let cwd=env::current_dir().unwrap_or_else(|_|PathBuf::from("."));
    if cwd.join("web/index.html").is_file(){cwd}else{env::current_exe().ok().and_then(|p|p.parent().map(Path::to_path_buf)).unwrap_or(cwd)}
}
fn reader_worker(state:Arc<Mutex<SharedState>>,schema:Option<Schema>,demo:bool,cache:PathBuf){
    let target=Duration::from_nanos(1_000_000_000/60);
    let begin=Instant::now();let mut proc=WinProcess::new();
    let mut reader=schema.map(CS2Reader::new);
    let mut tracker=BombTracker::default();
    let mut noise_tracker=NoiseTracker::default();
    let mut grenade_tracker=GrenadeTracker::default();
    let mut last_scan=Instant::now()-Duration::from_secs(10);
    let mut last_map=Instant::now()-Duration::from_secs(10);
    let mut last_detail=Instant::now()-Duration::from_secs(10);
    let mut last_grenade_scan=Instant::now()-Duration::from_secs(10);
    let mut last_frame=Instant::now();let mut rate=0.;
    let mut cache_details:HashMap<u32,cas_external_radar::model::Player>=HashMap::new();
    let mut cache_grenades:Vec<cas_external_radar::model::GrenadeView>=Vec::new();
    let mut last_log=Instant::now()-Duration::from_secs(12);
    let mut last_msg=String::new();
    loop {
        let tick=Instant::now();
        if demo {
            let mut s=demo_snapshot(begin.elapsed().as_secs_f64());noise_tracker.update(&mut s,Instant::now(),"de_mirage");
            if let Ok(mut st)=state.lock(){st.set_map("de_mirage","demo");st.snap=s;}
        }else {
            let result:Result<Snapshot,String>=(||{
                let reader=reader.as_mut().ok_or("No valid offsets; start once online or provide cache JSON")?;
                if last_scan.elapsed()>Duration::from_secs(5)||proc.pid==0{
                    last_scan=Instant::now();
                    if !proc.connect()?{return Ok(Snapshot::status("waiting","cs2.exe not found"));}
                }
                let base=proc.module("client.dll");let engine=proc.module("engine2.dll");
                if base==0{return Err("client.dll not loaded".into());}
                if last_map.elapsed()>=Duration::from_secs(1){
                    last_map=Instant::now();
                    if let Some(map)=reader.current_map(&mut proc,base){if let Ok(mut s)=state.lock(){s.set_map(&map,"memory");}}
                }
                let rich=last_detail.elapsed()>=Duration::from_millis(280);
                let grenade_scan=last_grenade_scan.elapsed()>=Duration::from_millis(90);
                let pid=proc.pid;let mut snap=reader.snapshot(&mut proc,base,engine,pid,rich,grenade_scan)?;
                if grenade_scan{last_grenade_scan=Instant::now();}
                if rich{
                    cache_details=snap.players.iter().map(|p|(p.id,p.clone())).collect();last_detail=Instant::now();
                }else{
                    for p in &mut snap.players{if let Some(cached)=cache_details.get(&p.id){
                        p.money=cached.money;p.armor=cached.armor;p.helmet=cached.helmet;p.defuser=cached.defuser;
                        p.weapon=cached.weapon.clone();p.inventory=cached.inventory.clone();p.has_c4=cached.has_c4;
                        p.agent_model=cached.agent_model.clone();
                    }}
                }
                if !grenade_scan{
                    snap.grenades=cache_grenades.clone();
                    let elapsed=last_grenade_scan.elapsed().as_secs_f32();
                    for g in &mut snap.grenades {if let Some(left)=g.remaining_secs.as_mut(){*left=(*left-elapsed).max(0.);}}
                }
                let map=state.lock().map(|s|s.current_map().to_owned()).unwrap_or_default();
                tracker.update(&mut snap,Instant::now(),&map);
                grenade_tracker.update(&mut snap,Instant::now(),&map);
                if grenade_scan{cache_grenades=snap.grenades.clone();}
                if let Some(m)=snap.local_motion.as_mut(){if m.weapon.is_none(){m.weapon=snap.players.iter().find(|p|p.local).map(|p|p.weapon.clone()).filter(|w|w!="—");}}
                noise_tracker.update(&mut snap,Instant::now(),&map);
                let elapsed=tick.duration_since(last_frame).as_secs_f64().max(0.001);
                rate=if rate==0. {1./elapsed}else{rate*0.83+0.17/elapsed};last_frame=tick;
                snap.source_hz=(rate*10.).round()/10.;Ok(snap)
            })();
            let snap=match result {
                Ok(s)=>s,
                Err(e)=>{proc.close();tracker.reset();grenade_tracker.reset();noise_tracker.reset();cache_grenades.clear();cache_details.clear();Snapshot::status("error",&e)},
            };
            if snap.status!="live"{
                let msg=format!("{}: {:?}",snap.status,snap.message);
                if last_msg!=msg||last_log.elapsed()>Duration::from_secs(10){
                    let stripped=json!({"status":snap.status.clone(),"message":snap.message.clone(),"diagnostics":snap.diagnostics.clone(),"pid":snap.pid,"time":snap.time});
                    eprintln!("[reader] {stripped}");
                    let _=fs::write(cache.join("diagnostics.json"),serde_json::to_vec_pretty(&stripped).unwrap_or_default());
                    last_log=Instant::now();last_msg=msg;
                }
            }
            if let Ok(mut st)=state.lock(){st.snap=snap;}
        }
        if tick.elapsed()<target{thread::sleep(target-tick.elapsed());}
    }
}
fn browser(url:&str){
    #[cfg(windows)]{let _=std::process::Command::new("cmd").args(["/C","start","",url]).spawn();}
    #[cfg(not(windows))]{let _=std::process::Command::new("xdg-open").arg(url).spawn();}
}
fn main(){if let Err(e)=start(){eprintln!("ERROR: {e}");std::process::exit(1);}}
fn start()->Result<(),String>{
    let args=parse_args()?;
    let root=exe_root();let cache=root.join("cache");fs::create_dir_all(&cache).map_err(|e|e.to_string())?;
    let maps=assets::load_maps(&cache,args.offline,args.refresh);
    let schema=if args.demo {None}else {
        let mut url=|name:&str|assets::cached_json(&format!("{}{}.json",assets::OFFSETS_SRC,name),&cache.join(format!("{}.json",name)),args.offline,true);
        let offsets=url("offsets");let client=url("client_dll");let info=url("info").unwrap_or(json!({}));
        match (offsets,client){(Some(o),Some(c))=>match Schema::from_json(&o,&c,info){Ok(s)=>Some(s),Err(e)=>{eprintln!("[schema] {e}");None}},_=>None}
    };
    let token=random_token(24);let secret_path=cache.join("gsi_token.txt");
    let gsi_token=fs::read_to_string(&secret_path).ok().map(|s|s.trim().to_string()).filter(|s|s.len()>25)
        .unwrap_or_else(||{let n=random_token(32);let _=fs::write(&secret_path,&n);n});
    if !args.demo&&!args.no_gsi_install{
        match gsi::install_cfg(args.port,&gsi_token,args.cfg.as_deref()){
            Ok((path,changed))=>{println!("[map] GSI: {}",path.display());if changed{println!("[map] Restart CS2 once to activate the new GSI config");}}
            Err(e)=>eprintln!("[map] GSI not installed: {e}; trying memory map fallback"),
        }
    }
    let state=Arc::new(Mutex::new(SharedState::new(maps)));
    let worker=state.clone();let demo=args.demo;let worker_cache=cache.clone();
    thread::spawn(move||reader_worker(worker,schema,demo,worker_cache));
    let overlay=OverlayController::spawn(state.clone(),cache.clone(),args.offline);
    let server=HttpServer {bind:if args.lan {"0.0.0.0"}else{"127.0.0.1"}.into(),port:args.port,
        token:token.clone(),gsi_token,cache:root.join("cache"),offline:args.offline,
        html:root.join("web/index.html"),state,overlay};
    let url=format!("http://127.0.0.1:{}/?token={}",args.port,token);
    println!("\nCAS EXTERNAL RADAR — RUST / read-only / no injection\n  PC: {url}\n");
    if args.lan {println!("  LAN: replace 127.0.0.1 with your PC's LAN IPv4, keep entire token.\n  Do not forward this port to the Internet.");}
    if !args.no_browser{browser(&url);}
    server.serve()
}