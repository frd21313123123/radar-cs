//! Optional Windows-native topmost mini radar. No browser/Chromium/WebView.
//! Its own Win32 message loop draws the map via GDI+ and follows SharedState.
use crate::state::SharedState;
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::sync::{Arc,Mutex};


/// Project a Source 2 world point into the Win32 map client area.
/// With follow=true and an available local position, local player is exactly
/// centered regardless of window size and zoom.
pub fn screen_point(position:[f32;3], overview:(f32,f32,f32), width:f32, height:f32,
    zoom:f32, center:Option<[f32;3]>)->Option<(f32,f32)> {
    let (pos_x,pos_y,scale)=overview;
    if ![pos_x,pos_y,scale,width,height,zoom].iter().all(|v|v.is_finite())
        || scale<=0.||width<=0.||height<=0.||zoom<=0.
        || !position.iter().all(|x|x.is_finite()) {return None;}
    let uv=|p:[f32;3]|((p[0]-pos_x)/(scale*1024.),(pos_y-p[1])/(scale*1024.));
    let (cx,cy)=center.filter(|p|p.iter().all(|x|x.is_finite())).map(uv).unwrap_or((0.5,0.5));
    let (u,v)=uv(position);
    let side=(width.min(height)-12.).max(1.)*zoom;
    Some((width/2.+(u-cx)*side,height/2.+(v-cy)*side))
}

#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct OverlaySettings {
    pub visible: bool,
    pub topmost: bool,
    pub center_local: bool,
    pub zoom: f32,
    pub opacity: u8,
    pub friend_color: String,
    pub enemy_color: String,
    pub self_color: String,
}
impl Default for OverlaySettings {
    fn default()->Self {Self { visible:false,topmost:true,center_local:true,
        zoom:1.5,opacity:100,friend_color:"#3b82f6".into(),
        enemy_color:"#ef4444".into(),self_color:"#22c55e".into()}}
}
impl OverlaySettings {
    fn update(&mut self, v:&Value)->Result<(),String> {
        let o=v.as_object().ok_or("Expected settings object")?;
        for key in o.keys() {if !["visible","topmost","center_local","zoom","opacity","friend_color","enemy_color","self_color"].contains(&key.as_str()) {return Err(format!("Unknown setting: {key}"));}}
        if let Some(x)=o.get("visible"){self.visible=x.as_bool().ok_or("visible must be boolean")?;}
        if let Some(x)=o.get("topmost"){self.topmost=x.as_bool().ok_or("topmost must be boolean")?;}
        if let Some(x)=o.get("center_local"){self.center_local=x.as_bool().ok_or("center_local must be boolean")?;}
        if let Some(x)=o.get("zoom"){let z=x.as_f64().ok_or("zoom must be numeric")?;if !(0.7..=4.).contains(&z){return Err("zoom outside 0.7-4".into());}self.zoom=z as f32;}
        if let Some(x)=o.get("opacity"){let z=x.as_u64().ok_or("opacity must be integer")?;if !(50..=100).contains(&z){return Err("opacity outside 50-100".into());}self.opacity=z as u8;}
        for (key,target) in [("friend_color",&mut self.friend_color),("enemy_color",&mut self.enemy_color),("self_color",&mut self.self_color)] {
            if let Some(x)=o.get(key){let col=x.as_str().ok_or("color must be string")?;
                if col.len()!=7||!col.starts_with('#')||!col[1..].bytes().all(|b|b.is_ascii_hexdigit()) {return Err(format!("Invalid {key}"));}
                *target=col.into();}
        }
        Ok(())
    }
}
#[derive(Clone)]pub struct OverlayController {settings:Arc<Mutex<OverlaySettings>>, #[cfg(windows)]inner:Arc<win::WindowBridge>, #[cfg(windows)]persist_path:std::path::PathBuf}
impl OverlayController {
    pub fn spawn(state:Arc<Mutex<SharedState>>,cache:std::path::PathBuf,offline:bool)->Self {
        #[cfg(windows)] let persist_path=cache.join("overlay_settings.json");
        #[cfg(windows)] let config=std::fs::read(&persist_path).ok().and_then(|v|serde_json::from_slice::<Value>(&v).ok())
            .and_then(|v|{let mut s=OverlaySettings::default();s.update(&v).ok().map(|_|s)}).unwrap_or_default();
        #[cfg(not(windows))] let config=OverlaySettings::default();
        let settings=Arc::new(Mutex::new(config));
        #[cfg(windows)] {
            let inner=win::WindowBridge::launch(settings.clone(),state,cache,offline);
            Self {settings,inner,persist_path}
        }
        #[cfg(not(windows))] {let _=(state,cache,offline);Self {settings}}
    }
    pub fn supported(&self)->bool{cfg!(windows)}
    pub fn status(&self)->Value {
        let settings=self.settings.lock().map(|s|s.clone()).unwrap_or_default();
        #[cfg(windows)] let window_ready=self.inner.hwnd.load(std::sync::atomic::Ordering::Acquire)!=0;
        #[cfg(not(windows))] let window_ready=false;
        json!({"supported":self.supported(),"window_ready":window_ready,"settings":settings})
    }
    pub fn update(&self,v:&Value)->Result<Value,String>{
        #[cfg(not(windows))]{let _=v;return Err("Native overlay requires Windows".into());}
        #[cfg(windows)] {
            let new={let mut locked=self.settings.lock().map_err(|_|"Overlay lock poisoned")?;
                let mut clone=locked.clone();clone.update(v)?;*locked=clone.clone();clone};
            self.inner.notify(&new);
            if let Ok(b)=serde_json::to_vec_pretty(&new){let _=std::fs::write(&self.persist_path,b);}
            Ok(self.status())
        }
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use crate::assets;
    use serde_json::Value;
    use std::{cell::RefCell,ffi::c_void,ptr::{null,null_mut},sync::{OnceLock,atomic::{AtomicIsize,Ordering}},thread,time::{Duration,Instant}};
    type HWND=*mut c_void;
    type HDC=*mut c_void;
    const WM_CLOSE:u32=0x10;const WM_DESTROY:u32=0x02;const WM_PAINT:u32=0x0F;const WM_ERASEBKGND:u32=0x14;
    const WM_TIMER:u32=0x113;const WM_MOUSEWHEEL:u32=0x20A;const WM_KEYDOWN:u32=0x100;const WM_APP_SETTINGS:u32=0x8001;
    const SW_HIDE:i32=0;const SW_SHOW:i32=5;
    const WS_OVERLAPPEDWINDOW:u32=0x00cf0000;const WS_EX_TOPMOST:u32=0x00000008;
    const WS_EX_LAYERED:u32=0x00080000;const SWP_NOMOVE:u32=0x0002;const SWP_NOSIZE:u32=0x0001;const SWP_NOACTIVATE:u32=0x0010;
    const LWA_ALPHA:u32=0x0002;const TRANSPARENT:i32=1;
    #[repr(C)]#[derive(Default,Clone,Copy)]struct Rect {left:i32,top:i32,right:i32,bottom:i32}
    #[repr(C)]#[derive(Default,Clone,Copy)]struct Point {x:i32,y:i32}
    #[repr(C)]struct Paint {hdc:HDC,erase:i32,rect:Rect,restore:i32,inc_update:i32,reserved:[u8;32]}
    impl Default for Paint {fn default()->Self{unsafe{std::mem::zeroed()}}}
    #[repr(C)]struct WindowClass {style:u32,proc:Option<unsafe extern "system" fn(HWND,u32,usize,isize)->isize>,cls_extra:i32,wnd_extra:i32,
        instance:HWND,icon:HWND,cursor:HWND,background:HWND,menu:*const u16,class:*const u16}
    #[repr(C)]struct Msg {hwnd:HWND,msg:u32,wparam:usize,lparam:isize,time:u32,pt:Point,lprivate:u32}
    impl Default for Msg {fn default()->Self{unsafe{std::mem::zeroed()}}}
    #[repr(C)]struct GdiStart {version:u32,callback:*mut c_void,suppress_background:i32,suppress_codecs:i32}
    #[link(name="user32")]
    extern "system" {
        fn RegisterClassW(class:*const WindowClass)->u16;
        fn LoadCursorW(instance:HWND,name:*const u16)->HWND;
        fn CreateWindowExW(ex:u32,class:*const u16,title:*const u16,style:u32,x:i32,y:i32,w:i32,h:i32,parent:HWND,menu:HWND,instance:HWND,param:*const c_void)->HWND;
        fn DefWindowProcW(hwnd:HWND,msg:u32,wparam:usize,lparam:isize)->isize;
        fn ShowWindow(hwnd:HWND,cmd:i32)->i32;fn UpdateWindow(hwnd:HWND)->i32;
        fn SetWindowPos(hwnd:HWND,after:HWND,x:i32,y:i32,w:i32,h:i32,flags:u32)->i32;
        fn SetLayeredWindowAttributes(hwnd:HWND,key:u32,alpha:u8,flags:u32)->i32;
        fn GetClientRect(hwnd:HWND,rect:*mut Rect)->i32;
        fn InvalidateRect(hwnd:HWND,rect:*const Rect,erase:i32)->i32;
        fn BeginPaint(hwnd:HWND,paint:*mut Paint)->HDC;fn EndPaint(hwnd:HWND,paint:*const Paint)->i32;
        fn SetTimer(hwnd:HWND,id:usize,period:u32,cb:*const c_void)->usize;
        fn KillTimer(hwnd:HWND,id:usize)->i32;
        fn GetMessageW(msg:*mut Msg,hwnd:HWND,low:u32,high:u32)->i32;
        fn TranslateMessage(msg:*const Msg)->i32;fn DispatchMessageW(msg:*const Msg)->isize;
        fn PostMessageW(hwnd:HWND,msg:u32,wparam:usize,lparam:isize)->i32;
        fn PostQuitMessage(code:i32);
    }
    #[link(name="kernel32")]extern "system" {fn GetModuleHandleW(module:*const u16)->HWND;}
    #[link(name="gdi32")]extern "system" {
        fn SetBkMode(dc:HDC,mode:i32)->i32;fn SetTextColor(dc:HDC,color:u32)->u32;
        fn TextOutW(dc:HDC,x:i32,y:i32,text:*const u16,len:i32)->i32;
    }
    #[link(name="gdiplus")]
    extern "system" {
        fn GdiplusStartup(token:*mut usize,input:*const GdiStart,output:*mut c_void)->i32;
        fn GdiplusShutdown(token:usize);
        fn GdipLoadImageFromFile(name:*const u16,image:*mut *mut c_void)->i32;
        fn GdipDisposeImage(image:*mut c_void)->i32;
        fn GdipCreateFromHDC(dc:HDC,graphics:*mut *mut c_void)->i32;
        fn GdipDeleteGraphics(graphics:*mut c_void)->i32;
        fn GdipGraphicsClear(graphics:*mut c_void,color:u32)->i32;
        fn GdipDrawImageRectI(graphics:*mut c_void,image:*mut c_void,x:i32,y:i32,w:i32,h:i32)->i32;
        fn GdipCreateSolidFill(color:u32,brush:*mut *mut c_void)->i32;
        fn GdipFillEllipseI(g:*mut c_void,brush:*mut c_void,x:i32,y:i32,w:i32,h:i32)->i32;
        fn GdipFillPolygonI(g:*mut c_void,brush:*mut c_void,points:*const Point,count:i32,mode:i32)->i32;
        fn GdipDeleteBrush(brush:*mut c_void)->i32;
    }
    static GLOBAL:OnceLock<Arc<WindowBridge>>=OnceLock::new();
    thread_local! {static CACHE:RefCell<NativeMap>=RefCell::new(NativeMap::default());
        static LAST_MAP_CHECK:RefCell<Option<Instant>>=const{RefCell::new(None)};}
    struct NativeMap {image:*mut c_void,name:String,last_try:Option<Instant>}
    impl Default for NativeMap {fn default()->Self{Self{image:null_mut(),name:String::new(),last_try:None}}}
    pub struct WindowBridge {pub settings:Arc<Mutex<OverlaySettings>>,pub state:Arc<Mutex<SharedState>>,pub cache:std::path::PathBuf,pub offline:bool,pub hwnd:AtomicIsize}
    impl WindowBridge {
        pub fn launch(settings:Arc<Mutex<OverlaySettings>>,state:Arc<Mutex<SharedState>>,cache:std::path::PathBuf,offline:bool)->Arc<Self>{
            let bridge=Arc::new(Self{settings,state,cache,offline,hwnd:AtomicIsize::new(0)});
            let _=GLOBAL.set(bridge.clone());
            thread::spawn(move||unsafe{run();});
            bridge
        }
        pub fn notify(&self,cfg:&OverlaySettings){let hwnd=self.hwnd.load(Ordering::Acquire);if hwnd!=0 {
            unsafe{let _=PostMessageW(hwnd as HWND,WM_APP_SETTINGS,0,0);}
        }let _=cfg;}
    }
    fn wide(s:&str)->Vec<u16>{s.encode_utf16().chain(std::iter::once(0)).collect()}
    fn brush(g:*mut c_void,color:u32)->*mut c_void {let mut b=null_mut();unsafe{GdipCreateSolidFill(color,&mut b);}let _=g;b}
    fn fill_circle(g:*mut c_void,x:i32,y:i32,r:i32,color:u32){unsafe{let b=brush(g,color);if !b.is_null(){GdipFillEllipseI(g,b,x-r,y-r,2*r,2*r);GdipDeleteBrush(b);}}}
    fn triangle(g:*mut c_void,x:f32,y:f32,angle:f32,color:u32){
        let (dx,dy)=(angle.cos(),-angle.sin());let side=(-dy,dx);
        let points=[Point{x:(x+dx*13.) as i32,y:(y+dy*13.) as i32},
            Point{x:(x-dx*6.+side.0*6.) as i32,y:(y-dy*6.+side.1*6.) as i32},
            Point{x:(x-dx*6.-side.0*6.) as i32,y:(y-dy*6.-side.1*6.) as i32}];
        unsafe{let b=brush(g,color);if !b.is_null(){GdipFillPolygonI(g,b,points.as_ptr(),3,0);GdipDeleteBrush(b);}}
    }
    fn argb(hex:&str)->u32 {let rgb=u32::from_str_radix(hex.strip_prefix('#').unwrap_or("3b82f6"),16).unwrap_or(0x3b82f6);0xff000000|rgb}
    unsafe fn load_map(bridge:&WindowBridge,name:&str,maps:&Value){
        CACHE.with(|cache|{let mut map=cache.borrow_mut();if map.name==name {return;}
            if map.last_try.map(|t|t.elapsed()<Duration::from_secs(3)).unwrap_or(false){return;}
            map.last_try=Some(Instant::now());
            // Map art remains an ordinary cached Valve overview PNG, decoded
            // by Windows GDI+, not an embedded browser or WebView.
            if let Ok(bytes)=assets::map_image(name,"default",maps,&bridge.cache,bridge.offline){
                let path=bridge.cache.join(format!("native_{name}.png"));
                if std::fs::write(&path,bytes).is_ok(){
                    let mut loaded=null_mut();
                    if GdipLoadImageFromFile(wide(&path.to_string_lossy()).as_ptr(),&mut loaded)==0 {
                        if !map.image.is_null(){GdipDisposeImage(map.image);}
                        map.image=loaded;map.name=name.into();
                    }
                }
            }
        });
    }
    fn overlay_frame(g:*mut c_void,dc:HDC,width:i32,height:i32,bridge:&WindowBridge){
        let snap=bridge.state.lock().ok().map(|st|st.snapshot());
        let (snap,mapdata,game_map)=if let Some(s)=snap {
            let st=bridge.state.lock().ok();let map=s.game_map.clone();let data=st.as_ref().map(|x|x.maps["maps"][&map]["radar_info"].clone()).unwrap_or(Value::Null);
            (s,data,map)
        }else{return};
        let cfg=bridge.settings.lock().map(|s|s.clone()).unwrap_or_default();
        unsafe{GdipGraphicsClear(g,0xff10171d);}
        let pos_x=mapdata["pos_x"].as_f64().unwrap_or(-3230.) as f32;
        let pos_y=mapdata["pos_y"].as_f64().unwrap_or(1713.) as f32;
        let scale=mapdata["scale"].as_f64().unwrap_or(5.) as f32;
        let local=snap.players.iter().find(|p|p.local&&p.life_state=="alive");
        let center=if cfg.center_local {local.map(|p|p.position)} else {None};
        let side=(width.min(height)-12).max(1) as f32*cfg.zoom;
        let (left,top)=screen_point([pos_x,pos_y,0.],(pos_x,pos_y,scale),width as f32,height as f32,cfg.zoom,center)
            .unwrap_or((0.,0.));
        CACHE.with(|image|{let map=image.borrow();if !map.image.is_null()&&map.name==game_map {
            unsafe{GdipDrawImageRectI(g,map.image,left as i32,top as i32,side as i32,side as i32);}
        }});
        for p in &snap.players {
            if p.life_state!="alive" {continue;}
            let (x,y)=match screen_point(p.position,(pos_x,pos_y,scale),width as f32,height as f32,cfg.zoom,center) {Some(v)=>v,None=>continue};
            if x < -20.||y < -20.||x > width as f32+20.||y > height as f32+20.{continue;}
            let col=if p.local {argb(&cfg.self_color)}else if p.team==snap.local_team {argb(&cfg.friend_color)}else {argb(&cfg.enemy_color)};
            fill_circle(g,x as i32,y as i32,9,0xff071018);
            if let Some(yaw)=p.yaw {triangle(g,x,y,yaw.to_radians(),col);}else{fill_circle(g,x as i32,y as i32,6,col);}
            if Some(p.id)==snap.bomb_carrier_id {fill_circle(g,x as i32,y as i32-18,4,0xffffcb70);}
        }
        // Native GDI text is drawn outside of GDI+ to avoid mixed HDC ownership.
        let text=if game_map.is_empty(){"CAS MINI • WAITING FOR MAP".to_owned()}else{format!("CAS MINI  •  {}",game_map.to_uppercase())};
        let _=dc;
        STATUS.with(|s|{*s.borrow_mut()=text;});
    }
    thread_local!{static STATUS:RefCell<String>=const{RefCell::new(String::new())};}
    unsafe fn update_window(hwnd:HWND){
        let bridge=match GLOBAL.get(){Some(b)=>b,None=>return};
        let cfg=bridge.settings.lock().map(|s|s.clone()).unwrap_or_default();
        SetLayeredWindowAttributes(hwnd,0,((cfg.opacity as u16*255)/100) as u8,LWA_ALPHA);
        SetWindowPos(hwnd,if cfg.topmost{-1isize as HWND}else{-2isize as HWND},0,0,0,0,SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE);
        if cfg.visible {ShowWindow(hwnd,SW_SHOW);}else{ShowWindow(hwnd,SW_HIDE);}
        if cfg.visible {InvalidateRect(hwnd,null(),0);UpdateWindow(hwnd);}
    }
    unsafe extern "system" fn wnd_proc(hwnd:HWND,msg:u32,wparam:usize,lparam:isize)->isize {
        match msg {
            WM_CLOSE=>{if let Some(b)=GLOBAL.get(){if let Ok(mut s)=b.settings.lock(){s.visible=false;
                let _=std::fs::write(b.cache.join("overlay_settings.json"),serde_json::to_vec_pretty(&*s).unwrap_or_default());
            }}ShowWindow(hwnd,SW_HIDE);0}
            WM_DESTROY=>{KillTimer(hwnd,1);PostQuitMessage(0);0}
            WM_ERASEBKGND=>1,
            WM_APP_SETTINGS=>{update_window(hwnd);0}
            WM_TIMER=>{
                if let Some(bridge)=GLOBAL.get(){
                    let visible=bridge.settings.lock().map(|s|s.visible).unwrap_or(false);
                    if visible {
                        let should_check=LAST_MAP_CHECK.with(|last|{
                            let mut t=last.borrow_mut();if t.map(|v|v.elapsed()>Duration::from_millis(400)).unwrap_or(true){*t=Some(Instant::now());true}else{false}
                        });
                        if should_check {
                            let (map,maps)=bridge.state.lock().ok().map(|s|(s.current_map().to_owned(),s.maps.clone())).unwrap_or_default();
                            if !map.is_empty(){load_map(bridge,&map,&maps);}
                        }
                        InvalidateRect(hwnd,null(),0);
                    }
                }0
            }
            WM_MOUSEWHEEL=>{if let Some(b)=GLOBAL.get(){let delta=((wparam>>16) as u16) as i16;
                if let Ok(mut cfg)=b.settings.lock(){cfg.zoom=(cfg.zoom * if delta>0 {1.10}else{0.90}).clamp(0.7,4.);}
            }InvalidateRect(hwnd,null(),0);0}
            WM_KEYDOWN=>{
                if let Some(b)=GLOBAL.get(){if let Ok(mut cfg)=b.settings.lock(){match wparam {
                    0x43=>cfg.center_local=!cfg.center_local, // C
                    0x54=>cfg.topmost=!cfg.topmost, // T
                    0x6b|0xbb=>cfg.zoom=(cfg.zoom*1.1).min(4.), // +
                    0x6d|0xbd=>cfg.zoom=(cfg.zoom*0.9).max(0.7), // -
                    _=>{}
                }}update_window(hwnd);}0
            }
            WM_PAINT=>{
                let mut paint=Paint::default();let dc=BeginPaint(hwnd,&mut paint);
                if !dc.is_null(){let mut rect=Rect::default();GetClientRect(hwnd,&mut rect);let mut g=null_mut();
                    if GdipCreateFromHDC(dc,&mut g)==0 {
                        if let Some(b)=GLOBAL.get(){overlay_frame(g,dc,rect.right-rect.left,rect.bottom-rect.top,b);}
                        GdipDeleteGraphics(g);
                    }
                    SetBkMode(dc,TRANSPARENT);SetTextColor(dc,0x00c9e5ee);
                    STATUS.with(|s|{let t=wide(&s.borrow());TextOutW(dc,12,8,t.as_ptr(),(t.len()-1) as i32);});
                }EndPaint(hwnd,&paint);0
            }
            _=>DefWindowProcW(hwnd,msg,wparam,lparam),
        }
    }
    unsafe fn run(){
        let mut gdip=0usize;
        if GdiplusStartup(&mut gdip,&GdiStart{version:1,callback:null_mut(),suppress_background:0,suppress_codecs:0},null_mut())!=0 {
            eprintln!("[overlay] GDI+ initialization failed");return;
        }
        let instance=GetModuleHandleW(null());let class=wide("CAS_RUST_NATIVE_RADAR_WINDOW");
        let wc=WindowClass{style:0,proc:Some(wnd_proc),cls_extra:0,wnd_extra:0,instance,icon:null_mut(),cursor:LoadCursorW(null_mut(),32512usize as *const u16),background:null_mut(),menu:null(),class:class.as_ptr()};
        if RegisterClassW(&wc)==0 {eprintln!("[overlay] RegisterClassW failed");GdiplusShutdown(gdip);return;}
        let title=wide("CAS Radar - Mini Map");
        let hwnd=CreateWindowExW(WS_EX_TOPMOST|WS_EX_LAYERED,class.as_ptr(),title.as_ptr(),WS_OVERLAPPEDWINDOW,100,100,370,370,null_mut(),null_mut(),instance,null());
        if hwnd.is_null(){eprintln!("[overlay] CreateWindowExW failed");GdiplusShutdown(gdip);return;}
        if let Some(b)=GLOBAL.get(){b.hwnd.store(hwnd as isize,Ordering::Release);}
        SetTimer(hwnd,1,16,null());
        // Initially hidden; browser exposes Open Mini Radar settings to show it.
        update_window(hwnd);
        let mut msg=Msg::default();while GetMessageW(&mut msg,null_mut(),0,0)>0{TranslateMessage(&msg);DispatchMessageW(&msg);}
        CACHE.with(|c|{let mut map=c.borrow_mut();if !map.image.is_null(){GdipDisposeImage(map.image);map.image=null_mut();}});
        GdiplusShutdown(gdip);
    }
}

#[cfg(test)]mod tests {
    use super::*;
    #[test]fn defaults(){let s=OverlaySettings::default();assert!(s.topmost&&s.center_local&&!s.visible);assert!((s.zoom-1.5).abs()<0.01);}
    #[test]fn validate(){let mut s=OverlaySettings::default();assert!(s.update(&json!({"zoom":2.0,"visible":true,"self_color":"#00ffbb"})).is_ok());assert!(s.visible);assert!((s.zoom-2.).abs()<0.01);assert!(s.update(&json!({"zoom":20})).is_err());assert!(s.update(&json!({"self_color":"red"})).is_err());}
    #[test]fn follow_centers_me(){let map=(-3230.,1713.,5.);let me=[-1500.,500.,5.];
        let p=screen_point(me,map,360.,500.,1.6,Some(me)).unwrap();
        assert!((p.0-180.).abs()<0.001 && (p.1-250.).abs()<0.001);
        let other=screen_point([-1000.,500.,0.],map,360.,500.,1.6,Some(me)).unwrap();
        assert!(other.0>180.);
    }
    #[test]fn invalid_projection(){assert!(screen_point([0.,0.,0.],(0.,0.,0.),320.,320.,1.,None).is_none());}
}