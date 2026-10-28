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


/// Rotate with the map, then retain distant markers at the visible edge.
/// Insets reserve space for the header and the readable player labels.
pub fn marker_point(point:(f32,f32),width:f32,height:f32,angle:f32)->Option<(f32,f32,bool)> {
    if ![point.0,point.1,width,height,angle].iter().all(|v|v.is_finite()) || width<100. || height<100. {return None;}
    let (cx,cy)=(width/2.,height/2.);
    let (sin,cos)=angle.to_radians().sin_cos();
    let (dx,dy)=(point.0-cx,point.1-cy);
    let (x,y)=(cx+dx*cos-dy*sin,cy+dx*sin+dy*cos);
    let (left,right,top,bottom)=(30.,width-30.,65.,height-62.);
    if x>=left&&x<=right&&y>=top&&y<=bottom{return Some((x,y,false));}
    let (dx,dy)=(x-cx,y-cy);
    let mut t=1.0f32;
    if dx>0. {t=t.min((right-cx)/dx);} else if dx<0. {t=t.min((left-cx)/dx);}
    if dy>0. {t=t.min((bottom-cy)/dy);} else if dy<0. {t=t.min((top-cy)/dy);}
    Some((cx+dx*t,cy+dy*t,true))
}

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(default)]
pub struct OverlaySettings {
    pub visible: bool,
    pub topmost: bool,
    pub center_local: bool,
    pub follow_spectator: bool,
    pub rotate_with_view: bool,
    pub show_noise: bool,
    pub show_sites: bool,
    pub show_smokes: bool,
    pub show_fire: bool,
    pub show_paths: bool,
    pub show_bomb_timer: bool,
    pub zoom: f32,
    pub opacity: u8,
    pub friend_color: String,
    pub enemy_color: String,
    pub self_color: String,
}
impl Default for OverlaySettings {
    fn default()->Self {Self { visible:false,topmost:true,center_local:true,follow_spectator:true,rotate_with_view:true,show_noise:true,show_sites:true,show_smokes:true,show_fire:true,show_paths:true,show_bomb_timer:true,
        zoom:1.0,opacity:100,friend_color:"#3b82f6".into(),
        enemy_color:"#ef4444".into(),self_color:"#22c55e".into()}}
}
impl OverlaySettings {
    fn update(&mut self, v:&Value)->Result<(),String> {
        let o=v.as_object().ok_or("Expected settings object")?;
        for key in o.keys() {if !["visible","topmost","center_local","follow_spectator","rotate_with_view","show_noise","show_sites","show_smokes","show_fire","show_paths","show_bomb_timer","zoom","opacity","friend_color","enemy_color","self_color"].contains(&key.as_str()) {return Err(format!("Unknown setting: {key}"));}}
        if let Some(x)=o.get("visible"){self.visible=x.as_bool().ok_or("visible must be boolean")?;}
        if let Some(x)=o.get("topmost"){self.topmost=x.as_bool().ok_or("topmost must be boolean")?;}
        if let Some(x)=o.get("center_local"){self.center_local=x.as_bool().ok_or("center_local must be boolean")?;}
        if let Some(x)=o.get("follow_spectator"){self.follow_spectator=x.as_bool().ok_or("follow_spectator must be boolean")?;}
        if let Some(x)=o.get("rotate_with_view"){self.rotate_with_view=x.as_bool().ok_or("rotate_with_view must be boolean")?;}
        if let Some(x)=o.get("show_noise"){self.show_noise=x.as_bool().ok_or("show_noise must be boolean")?;}
        if let Some(x)=o.get("show_sites"){self.show_sites=x.as_bool().ok_or("show_sites must be boolean")?;}
        if let Some(x)=o.get("show_smokes"){self.show_smokes=x.as_bool().ok_or("show_smokes must be boolean")?;}
        if let Some(x)=o.get("show_fire"){self.show_fire=x.as_bool().ok_or("show_fire must be boolean")?;}
        if let Some(x)=o.get("show_paths"){self.show_paths=x.as_bool().ok_or("show_paths must be boolean")?;}
        if let Some(x)=o.get("show_bomb_timer"){self.show_bomb_timer=x.as_bool().ok_or("show_bomb_timer must be boolean")?;}
        if let Some(x)=o.get("zoom"){let z=x.as_f64().ok_or("zoom must be numeric")?;if !(0.35..=4.).contains(&z){return Err("zoom outside 0.35-4".into());}self.zoom=z as f32;}
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
    const WM_CLOSE:u32=0x10;const WM_DESTROY:u32=0x02;const WM_PAINT:u32=0x0F;const WM_ERASEBKGND:u32=0x14;const WM_GETMINMAXINFO:u32=0x24;
    const WM_TIMER:u32=0x113;const WM_MOUSEWHEEL:u32=0x20A;const WM_KEYDOWN:u32=0x100;const WM_APP_SETTINGS:u32=0x8001;
    const SW_HIDE:i32=0;const SW_SHOW:i32=5;
    const WS_POPUP:u32=0x80000000;const WS_THICKFRAME:u32=0x00040000;
    const WM_NCCALCSIZE:u32=0x83;const WM_NCHITTEST:u32=0x84;const WM_LBUTTONUP:u32=0x202;const WS_EX_TOPMOST:u32=0x00000008;
    const WS_EX_LAYERED:u32=0x00080000;const SWP_NOMOVE:u32=0x0002;const SWP_NOSIZE:u32=0x0001;const SWP_NOACTIVATE:u32=0x0010;
    const LWA_ALPHA:u32=0x0002;const TRANSPARENT:i32=1;
    const SRCCOPY:u32=0x00CC0020; // CAS_ANTI_FLICKER_V11
    #[repr(C)]#[derive(Default,Clone,Copy)]struct Rect {left:i32,top:i32,right:i32,bottom:i32}
    #[repr(C)]#[derive(Default,Clone,Copy)]struct Point {x:i32,y:i32}
    #[repr(C)]struct MinMaxInfo{reserved:Point,max_size:Point,max_position:Point,min_track_size:Point,max_track_size:Point}
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
        fn GetWindowRect(hwnd:HWND,rect:*mut Rect)->i32;
        fn ScreenToClient(hwnd:HWND,point:*mut Point)->i32;
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
        fn GetTextExtentPoint32W(dc:HDC,text:*const u16,len:i32,size:*mut Point)->i32;
        fn CreateFontW(height:i32,width:i32,escapement:i32,orientation:i32,weight:i32,
            italic:u32,underline:u32,strike:u32,charset:u32,out_precision:u32,
            clip_precision:u32,quality:u32,pitch:u32,face:*const u16)->*mut c_void;
        fn CreateCompatibleDC(dc:HDC)->HDC;
        fn CreateCompatibleBitmap(dc:HDC,width:i32,height:i32)->*mut c_void;
        fn SelectObject(dc:HDC,obj:*mut c_void)->*mut c_void;
        fn DeleteObject(obj:*mut c_void)->i32;
        fn DeleteDC(dc:HDC)->i32;
        fn BitBlt(dest:HDC,x:i32,y:i32,width:i32,height:i32,src:HDC,src_x:i32,src_y:i32,rop:u32)->i32;
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
        fn GdipFillRectangleI(g:*mut c_void,brush:*mut c_void,x:i32,y:i32,w:i32,h:i32)->i32;
        fn GdipDrawArcI(g:*mut c_void,pen:*mut c_void,x:i32,y:i32,w:i32,h:i32,start_angle:f32,sweep_angle:f32)->i32;
        fn GdipFillPolygonI(g:*mut c_void,brush:*mut c_void,points:*const Point,count:i32,mode:i32)->i32;
        fn GdipDeleteBrush(brush:*mut c_void)->i32;
        fn GdipCreatePen1(color:u32,width:f32,unit:i32,pen:*mut *mut c_void)->i32;
        fn GdipDrawEllipseI(g:*mut c_void,pen:*mut c_void,x:i32,y:i32,w:i32,h:i32)->i32;
        fn GdipDeletePen(pen:*mut c_void)->i32;
        fn GdipDrawLineI(g:*mut c_void,pen:*mut c_void,x1:i32,y1:i32,x2:i32,y2:i32)->i32;
        fn GdipSetPenDashStyle(pen:*mut c_void,style:i32)->i32;
        fn GdipDrawLinesI(g:*mut c_void,pen:*mut c_void,points:*const Point,count:i32)->i32;
        fn GdipTranslateWorldTransform(graphics:*mut c_void,dx:f32,dy:f32,order:i32)->i32;
        fn GdipRotateWorldTransform(graphics:*mut c_void,angle:f32,order:i32)->i32;
        fn GdipResetWorldTransform(graphics:*mut c_void)->i32;
    }
    static GLOBAL:OnceLock<Arc<WindowBridge>>=OnceLock::new();
    thread_local! {static CAMERA_YAW:RefCell<Option<(Instant,f32)>>=RefCell::new(None);
        static CAMERA_FOCUS_ID:RefCell<Option<(bool,u32)>>=RefCell::new(None);}
    fn smooth_yaw(target:f32)->f32 {
        CAMERA_YAW.with(|cell|{
            let mut state=cell.borrow_mut();let now=Instant::now();
            let angle=if let Some((at,prev))=*state {
                let delta=(target-prev+540.0).rem_euclid(360.0)-180.0;
                let secs=now.duration_since(at).as_secs_f32().min(0.2);
                let weight=1.0-(-12.0*secs).exp();
                prev+delta*weight
            }else{target};
            *state=Some((now,angle));angle
        })
    }
    thread_local! {static BOMB_ICON:RefCell<Option<*mut c_void>>=RefCell::new(None);}
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
    /// Vector badge: rendered under GDI+ world transform, staying attached to
    /// the overview image when the mini-radar follows or rotates.
    fn draw_site(g:*mut c_void,x:i32,y:i32,site:char){
        unsafe {
            fill_circle(g,x,y,12,0xe7251d14);
            let mut outline=null_mut();
            if GdipCreatePen1(0xfff4c174,1.7,2,&mut outline)==0 && !outline.is_null() {
                GdipDrawEllipseI(g,outline,x-12,y-12,24,24);
                GdipDeletePen(outline);
            }
            let mut pen=null_mut();
            if GdipCreatePen1(0xfffaebc3,2.3,2,&mut pen)!=0||pen.is_null(){return;}
            // A and B are drawn as simple line glyphs (no font dependencies).
            let lines:&[(i32,i32,i32,i32)]=if site=='A'{
                &[(-6,7,0,-7),(0,-7,6,7),(-4,2,4,2)]
            }else{
                &[(-5,-7,-5,7),(-5,-7,2,-7),(2,-7,5,-4),(5,-4,5,-1),(5,-1,2,0),
                  (-5,0,2,0),(2,0,6,3),(6,3,5,5),(5,5,2,7),(2,7,-5,7)]
            };
            for &(x1,y1,x2,y2) in lines{GdipDrawLineI(g,pen,x+x1,y+y1,x+x2,y+y2);}
            GdipDeletePen(pen);
        }
    }

    fn draw_dropped_c4(g:*mut c_void,x:i32,y:i32,cache:&std::path::Path){
        let mut rendered=false;
        BOMB_ICON.with(|slot|{
            let mut item=slot.borrow_mut();
            if item.is_none(){
                let file=cache.join("native_ground_c4_v17.png");
                let _=std::fs::write(&file,include_bytes!("../assets/bomb_ground.png"));
                let path=wide(&file.to_string_lossy());
                let mut image=null_mut();
                unsafe{if GdipLoadImageFromFile(path.as_ptr(),&mut image)!=0{image=null_mut();}}
                *item=Some(image);
            }
            if let Some(image)=*item{
                if !image.is_null(){
                    unsafe{GdipDrawImageRectI(g,image,x-18,y-18,36,36);}
                    rendered=true;
                }
            }
        });
        if !rendered {fill_circle(g,x,y,11,0xff1d1610);fill_circle(g,x,y,8,0xffffbc43);}
    }
    /// Styled after the supplied compact CS2 panel: dark backdrop, cyan ring.
    /// Called AFTER the world transform is reset, so the panel does not rotate.
    fn draw_bomb_panel(g:*mut c_void,remaining:f32,total:f32){
        if !remaining.is_finite()||!total.is_finite()||total<=0.0{return;}
        unsafe{
            let b=brush(g,0xf40b121b);
            if !b.is_null(){GdipFillRectangleI(g,b,9,35,241,91);GdipDeleteBrush(b);}
            let mut border=null_mut();
            if GdipCreatePen1(0xff395361,1.4,2,&mut border)==0&&!border.is_null(){
                GdipDrawEllipseI(g,border,21,53,57,57);GdipDeletePen(border);
            }
            let color=if remaining<=5.0{0xffff6170}else if remaining<=10.0{0xffffb45b}else{0xff25c5ef};
            let mut arc=null_mut();
            if GdipCreatePen1(color,5.0,2,&mut arc)==0&&!arc.is_null(){
                GdipDrawArcI(g,arc,21,53,57,57,-90.0,360.0*(remaining/total).clamp(0.0,1.0));
                GdipDeletePen(arc);
            }
        }
    }
    #[derive(Clone)]
    struct Label{x:i32,y:i32,text:String,color:u32,center:bool}
    thread_local!{static LABELS:RefCell<Vec<Label>>=const{RefCell::new(Vec::new())};}
    fn label(x:f32,y:f32,text:String,color:u32){
        LABELS.with(|labels|labels.borrow_mut().push(Label{x:x.round() as i32,y:y.round() as i32,text,color,center:true}));
    }
    fn rectangle(g:*mut c_void,x:i32,y:i32,w:i32,h:i32,color:u32){unsafe{
        let b=brush(g,color);if !b.is_null(){GdipFillRectangleI(g,b,x,y,w,h);GdipDeleteBrush(b);}
    }}
    fn segments(g:*mut c_void,lines:&[(i32,i32,i32,i32)],color:u32,width:f32){unsafe{
        let mut pen=null_mut();if GdipCreatePen1(color,width,2,&mut pen)==0&&!pen.is_null(){
            for &(x1,y1,x2,y2) in lines{GdipDrawLineI(g,pen,x1,y1,x2,y2);}
            GdipDeletePen(pen);
        }
    }}
    fn height_arrow(g:*mut c_void,x:i32,y:i32,level:i8){
        if level==0{return;}
        let dy=if level>0{-1}else{1};
        segments(g,&[(x,y-7*dy,x,y+7*dy),(x-4,y+3*dy,x,y+7*dy),(x+4,y+3*dy,x,y+7*dy)],0xfff2f7fc,2.4);
    }
    fn flash_cross(g:*mut c_void,x:i32,y:i32){
        // Two crossed white arrows remain recognizable without color vision.
        segments(g,&[(x-10,y-10,x+10,y+10),(x+4,y+10,x+10,y+10),(x+10,y+4,x+10,y+10),
            (x+10,y-10,x-10,y+10),(x-10,y+4,x-10,y+10),(x-4,y+10,x-10,y+10)],0xffffffff,2.8);
    }
    fn weapon_glyph(g:*mut c_void,x:i32,y:i32,weapon:&str){
        if weapon=="—"||weapon.is_empty(){return;}
        let color=0xffe7f0f8;
        if matches!(weapon,"Flashbang"|"Smoke"|"HE Grenade"|"Molotov"|"Incendiary"|"Decoy"){
            rectangle(g,x-5,y-4,10,9,color);segments(g,&[(x-2,y-7,x+3,y-7),(x+3,y-7,x+6,y-3)],color,2.);
        }else if weapon=="Knife"{
            segments(g,&[(x-12,y+4,x+12,y-4),(x-7,y+7,x-3,y-1),(x-3,y-1,x+12,y-4)],color,2.4);
        }else if weapon=="C4"{
            rectangle(g,x-9,y-5,18,10,color);rectangle(g,x-4,y-3,8,5,0xff24313e);
        }else{
            let pistol=matches!(weapon,"Glock-18"|"USP-S"|"USP-Silencer"|"P250"|"Five-SeveN"|"Tec-9"|"CZ75-Auto"|"Desert Eagle"|"Dual Berettas"|"R8 Revolver"|"P2000");
            let sniper=matches!(weapon,"AWP"|"SSG 08"|"SCAR-20"|"G3SG1");
            rectangle(g,x-9,y-3,if pistol{14}else{20},5,color);
            segments(g,&[(x-6,y,x-8,y+6),(x+5,y-2,x+(if pistol{11}else{19}),y-2)],color,2.3);
            if !pistol{segments(g,&[(x-10,y-1,x-17,y+3),(x-17,y+3,x-17,y-3),(x-1,y+2,x+3,y+7)],color,2.5);}
            if sniper{rectangle(g,x-3,y-7,12,3,color);}
        }
    }
    fn overlay_frame(g:*mut c_void,dc:HDC,width:i32,height:i32,bridge:&WindowBridge){
        LABELS.with(|labels|labels.borrow_mut().clear());
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
        let focus=snap.radar_focus(cfg.follow_spectator);
        // On spectator target changes, immediately adopt the new camera
        // orientation instead of smoothing from the previous player's yaw.
        let focus_id=focus.as_ref().map(|p|(p.spectating,p.entity_id));
        CAMERA_FOCUS_ID.with(|cell|{
            let mut last=cell.borrow_mut();
            if *last!=focus_id {CAMERA_YAW.with(|yaw|*yaw.borrow_mut()=None);}
            *last=focus_id;
        });
        let center=if cfg.center_local {focus.as_ref().map(|p|p.position)} else {None};
        // Source 2 yaw: 0 degrees east, 90 degrees north.
        // Map and players are transformed around the current observed target.
        let camera_angle=if cfg.rotate_with_view {
            focus.as_ref().and_then(|p|p.yaw).filter(|v|v.is_finite()).map(|yaw|smooth_yaw(yaw-90.0))
        }else{None};
        if camera_angle.is_none(){CAMERA_YAW.with(|c|*c.borrow_mut()=None);}
        if let Some(angle)=camera_angle {
            unsafe{
                GdipTranslateWorldTransform(g,-(width as f32)*0.5,-(height as f32)*0.5,1);
                GdipRotateWorldTransform(g,angle,1);
                GdipTranslateWorldTransform(g,(width as f32)*0.5,(height as f32)*0.5,1);
            }
        }
        let side=(width.min(height)-12).max(1) as f32*cfg.zoom;
        let (left,top)=screen_point([pos_x,pos_y,0.],(pos_x,pos_y,scale),width as f32,height as f32,cfg.zoom,center)
            .unwrap_or((0.,0.));
        CACHE.with(|image|{let map=image.borrow();if !map.image.is_null()&&map.name==game_map {
            unsafe{GdipDrawImageRectI(g,map.image,left as i32,top as i32,side as i32,side as i32);}
        }});
        if cfg.show_noise {
            if let (Some(me),Some(noise))=(local,snap.noise.as_ref()){
                if let Some((x,y))=screen_point(me.position,(pos_x,pos_y,scale),
                    width as f32,height as f32,cfg.zoom,center){
                    let r=(noise.radius_units/(scale*1024.0)*side).clamp(1.0,8000.0);
                    if r.is_finite()&&r>1.0 {unsafe{
                        let fill=brush(g,0x1bffbf69);
                        if !fill.is_null(){GdipFillEllipseI(g,fill,(x-r) as i32,(y-r) as i32,(r*2.0) as i32,(r*2.0) as i32);GdipDeleteBrush(fill);}
                        let mut pen=null_mut();
                        if GdipCreatePen1(0xdfffc776,2.0,2,&mut pen)==0&&!pen.is_null(){
                            GdipDrawEllipseI(g,pen,(x-r) as i32,(y-r) as i32,(r*2.0) as i32,(r*2.0) as i32);
                            GdipDeletePen(pen);
                        }
                    }}
                }
            }
        }
        if cfg.show_sites {
            if let Some(sites)=assets::bombsites_for_map(&game_map){
                for (i,[u,v]) in sites.into_iter().enumerate(){
                    let x=left+side*u;let y=top+side*v;
                    if x.is_finite()&&y.is_finite()&&x>=-30.&&y>=-30.&&x<=width as f32+30.&&y<=height as f32+30.{
                        draw_site(g,x.round() as i32,y.round() as i32,if i==0{'A'}else{'B'});
                    }
                }
            }
        }
        // Draw world C4 regardless of team filters; keep it in map space,
        // so it follows the same rotation and pan as players and plant sites.
        if let Some(bomb)=snap.bomb_world.as_ref(){
            if let Some((x,y))=screen_point(bomb.position,(pos_x,pos_y,scale),width as f32,height as f32,cfg.zoom,center){
                let px=x.round() as i32;let py=y.round() as i32;

                if bomb.status=="dropped" {draw_dropped_c4(g,px,py,&bridge.cache);}
                else {fill_circle(g,px,py,11,0xff261216);fill_circle(g,px,py,8,0xfffa514e);fill_circle(g,px,py,3,0xfff9deac);}
            }
        }
        // Utility overlays are drawn in map coordinates: camera rotation is
        // already active here, so paths and radii follow the map correctly.
        for nade in &snap.grenades{
            let zone_enabled=(nade.kind=="smoke"&&cfg.show_smokes)||(nade.kind=="molotov"&&cfg.show_fire);
            if !zone_enabled&&!cfg.show_paths{continue;}
            if cfg.show_paths && nade.trajectory.len()>1{
                let points:Vec<Point>=nade.trajectory.iter().filter_map(|v|
                    screen_point(*v,(pos_x,pos_y,scale),width as f32,height as f32,cfg.zoom,center)
                    .map(|(x,y)|Point{x:x.round() as i32,y:y.round() as i32})).collect();
                if points.len()>1{unsafe{
                    let mut pen=null_mut();
                    let ink=match nade.kind.as_str(){"smoke"=>0xff9ee6ed,"molotov"=>0xffffa268,
                        "flash"=>0xfff6f7ff,"he"=>0xfff66a68,"decoy"=>0xff8def88,_=>0xffc5cbd1};
                    if GdipCreatePen1(ink,2.0,2,&mut pen)==0&&!pen.is_null(){
                        GdipSetPenDashStyle(pen,2);GdipDrawLinesI(g,pen,points.as_ptr(),points.len() as i32);GdipDeletePen(pen);
                    }
                }}
            }
            let at=screen_point(nade.position,(pos_x,pos_y,scale),width as f32,height as f32,cfg.zoom,center);
            if let Some((x,y))=at{
                if nade.state=="active"&&zone_enabled{
                    let r=(nade.radius_units/(scale*1024.0)*side).clamp(1.0,2000.0);
                    let fire=nade.kind=="molotov";
                    let color=if fire{0x34ff763c}else{0x307bbbd2};
                    let outline=if fire{0xfffa9253}else{0xffa3e6f3};
                    unsafe{
                        let b=brush(g,color);
                        if !b.is_null(){GdipFillEllipseI(g,b,(x-r) as i32,(y-r) as i32,(2.*r) as i32,(2.*r) as i32);GdipDeleteBrush(b);}
                        let mut pen=null_mut();
                        if GdipCreatePen1(outline,1.8,2,&mut pen)==0&&!pen.is_null(){
                            GdipDrawEllipseI(g,pen,(x-r) as i32,(y-r) as i32,(r*2.) as i32,(r*2.) as i32);
                            GdipDeletePen(pen);
                        }
                    }
                    if fire{
                        for cell in &nade.fire_cells {
                            if let Some((fx,fy))=screen_point(*cell,(pos_x,pos_y,scale),width as f32,height as f32,cfg.zoom,center){
                                let cr=(38.0/(scale*1024.0)*side).max(2.0);
                                unsafe{let b=brush(g,0x5fff6a36);
                                    if !b.is_null(){GdipFillEllipseI(g,b,(fx-cr) as i32,(fy-cr) as i32,(2.*cr) as i32,(2.*cr) as i32);GdipDeleteBrush(b);}
                                }
                            }
                        }
                    }
                }else if nade.state=="flying"&&cfg.show_paths{
                    let ink=match nade.kind.as_str(){"smoke"=>0xff9ee6ed,"molotov"=>0xffffa268,
                        "flash"=>0xfff6f7ff,"he"=>0xfff66a68,"decoy"=>0xff8def88,_=>0xffc5cbd1};
                    fill_circle(g,x.round() as i32,y.round() as i32,5,ink);
                }
            }
        }
        // Reset first: labels, weapon icons and vertical arrows stay upright.
        if camera_angle.is_some(){unsafe{GdipResetWorldTransform(g);}}
        let age=(crate::model::now_secs()-snap.time).max(0.0) as f32;
        for nade in &snap.grenades{
            let enabled=(nade.kind=="smoke"&&cfg.show_smokes)||(nade.kind=="molotov"&&cfg.show_fire);
            if nade.state!="active"||!enabled{continue;}
            if let Some(remaining)=nade.remaining_secs.filter(|n|n.is_finite()).map(|n|(n-age).max(0.)){
                if remaining<=0.{continue;}
                let at=screen_point(nade.position,(pos_x,pos_y,scale),width as f32,height as f32,cfg.zoom,center)
                    .and_then(|p|marker_point(p,width as f32,height as f32,camera_angle.unwrap_or(0.)));
                if let Some((x,y,false))=at{
                    rectangle(g,x as i32-23,y as i32-11,46,22,0xef101820);
                    label(x,y-9.,format!("{}{remaining:.1}",if nade.timer_estimated{"~"}else{""}),0xfff3f7fa);
                }
            }
        }
        for p in &snap.players {
            if p.life_state!="alive" {continue;}
            let raw=match screen_point(p.position,(pos_x,pos_y,scale),width as f32,height as f32,cfg.zoom,center){Some(v)=>v,None=>continue};
            let (x,y,edge)=match marker_point(raw,width as f32,height as f32,camera_angle.unwrap_or(0.)){Some(v)=>v,None=>continue};
            let col=if p.local {argb(&cfg.self_color)}else if p.team==snap.local_team {argb(&cfg.friend_color)}else {argb(&cfg.enemy_color)};
            fill_circle(g,x as i32,y as i32,10,0xff071018);
            let yaw=if edge {
                // The edge chevron points to the actual offscreen location.
                let (sin,cos)=camera_angle.unwrap_or(0.).to_radians().sin_cos();
                let (dx,dy)=(raw.0-width as f32/2.,raw.1-height as f32/2.);
                Some((-(dx*sin+dy*cos)).atan2(dx*cos-dy*sin))
            }else{p.yaw.map(|yaw|(yaw-camera_angle.unwrap_or(0.)).to_radians())};
            if let Some(yaw)=yaw{triangle(g,x,y,yaw,col);}else{fill_circle(g,x as i32,y as i32,6,col);}
            if edge{label(x,y-30.,"ДАЛЕКО".into(),0xffc7d9e6);}
            if Some(p.id)==snap.bomb_carrier_id {fill_circle(g,x as i32-17,y as i32-15,4,0xffffcb70);}
            if !p.local&&p.team!=snap.local_team{
                height_arrow(g,x as i32+18,y as i32,crate::model::relative_height(p.position,focus.as_ref().map(|p|p.position)));
            }
            if p.is_flashed.unwrap_or(p.flash_remaining_secs.is_some_and(|n|n>0.)) {flash_cross(g,x as i32,y as i32);}
            rectangle(g,x as i32-17,y as i32+13,34,4,0xff15202b);
            if let Some(hp)=p.health{
                rectangle(g,x as i32-17,y as i32+13,(34.*(hp as f32/100.).clamp(0.,1.)) as i32,4,if hp<=30{0xffff7373}else{0xff80e5b2});
                label(x,y+18.,format!("{hp} HP"),0xfff4f8fc);
            }else{label(x,y+18.,"? HP".into(),0xffb2c3cf);}
            if p.weapon!="—"&&!p.weapon.is_empty(){
                // Keep the exact weapon name alongside a compact silhouette.
                weapon_glyph(g,x as i32,y as i32+38,&p.weapon);
                label(x,y+45.,p.weapon.clone(),0xffd9e8f1);
            }
        }
        // Custom chrome is drawn last and never rotates with the map.
        rectangle(g,0,0,width,34,0xf5101822);
        rectangle(g,width-40,4,34,30,0xff34232c);
        segments(g,&[(width-29,13,width-17,25),(width-17,13,width-29,25)],0xffffdce3,2.);
        let bomb_lines=if cfg.show_bomb_timer{
            snap.bomb_timer.as_ref().and_then(|timer|{
                let age=(crate::model::now_secs()-snap.time).clamp(0.0,1.5) as f32;
                let remaining=(timer.remaining_secs-age).max(0.0);
                if remaining<=0.0 {return None;}
                draw_bomb_panel(g,remaining,timer.total_secs);
                Some([format!("Плэнт: {}",timer.site.as_deref().unwrap_or("?")),
                    format!("{remaining:.1} сек")])
            })
        }else{None};
        BOMB_LINES.with(|s|*s.borrow_mut()=bomb_lines);
        // Native GDI text is drawn outside of GDI+ to avoid mixed HDC ownership.
        let mut text=if game_map.is_empty(){"CAS MINI • WAITING FOR MAP".to_owned()}else{format!("CAS MINI  •  {}",game_map.to_uppercase())};
        if cfg.follow_spectator {
            if let Some(target)=focus.as_ref().filter(|p|p.spectating){
                let short:String=target.name.chars().take(12).collect();
                text.push_str(&format!(" | OBS: {short}"));
            }
        }
        // Leave room for the independent close control at every window size.
        let max_chars=((width-62)/7).max(4) as usize;
        text=text.chars().take(max_chars).collect();
        let _=dc;
        STATUS.with(|s|{*s.borrow_mut()=text;});
    }
    thread_local!{static STATUS:RefCell<String>=const{RefCell::new(String::new())};}
    thread_local!{static BOMB_LINES:RefCell<Option<[String;2]>>=const{RefCell::new(None)};}
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
            // Remove Windows non-client chrome while retaining native resizing.
            WM_NCCALCSIZE=>0,
            WM_NCHITTEST=>{
                let mut point=Point{x:(lparam as u16 as i16) as i32,y:((lparam>>16) as u16 as i16) as i32};
                let mut frame=Rect::default();GetWindowRect(hwnd,&mut frame);
                let (left,right,top,bottom)=(point.x<frame.left+6,point.x>=frame.right-6,point.y<frame.top+6,point.y>=frame.bottom-6);
                if top&&left{return 13;}if top&&right{return 14;}
                if bottom&&left{return 16;}if bottom&&right{return 17;}
                if left{return 10;}if right{return 11;}if top{return 12;}if bottom{return 15;}
                ScreenToClient(hwnd,&mut point);let mut client=Rect::default();GetClientRect(hwnd,&mut client);
                if point.y<34&&point.x<client.right-40 {2} else {1}
            }
            WM_LBUTTONUP=>{
                let x=(lparam as u16 as i16) as i32;let y=((lparam>>16) as u16 as i16) as i32;
                let mut rect=Rect::default();GetClientRect(hwnd,&mut rect);
                if x>=rect.right-40&&x<rect.right-6&&(4..34).contains(&y){PostMessageW(hwnd,WM_CLOSE,0,0);}
                0
            }
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
                if let Ok(mut cfg)=b.settings.lock(){cfg.zoom=(cfg.zoom * if delta>0 {1.10}else{0.90}).clamp(0.35,4.);}
            }InvalidateRect(hwnd,null(),0);0}
            WM_KEYDOWN=>{
                if let Some(b)=GLOBAL.get(){if let Ok(mut cfg)=b.settings.lock(){match wparam {
                    0x1b=>{drop(cfg);PostMessageW(hwnd,WM_CLOSE,0,0);return 0;}, // Escape
                    0x43=>cfg.center_local=!cfg.center_local, // C
                    0x54=>cfg.topmost=!cfg.topmost, // T
                    0x6b|0xbb=>cfg.zoom=(cfg.zoom*1.1).min(4.), // +
                    0x6d|0xbd=>cfg.zoom=(cfg.zoom*0.9).max(0.35), // -
                    _=>{}
                }}update_window(hwnd);}0
            }
            WM_GETMINMAXINFO=>{
                if lparam!=0{
                    let info=&mut *(lparam as *mut MinMaxInfo);
                    info.min_track_size=Point{x:295,y:205};
                }
                0
            }
            WM_PAINT=>{
                let mut paint=Paint::default();let screen=BeginPaint(hwnd,&mut paint);
                if !screen.is_null(){
                    let mut rect=Rect::default();GetClientRect(hwnd,&mut rect);
                    let w=(rect.right-rect.left).max(1);
                    let h=(rect.bottom-rect.top).max(1);
                    let mem=CreateCompatibleDC(screen);
                    if !mem.is_null(){
                        let bmp=CreateCompatibleBitmap(screen,w,h);
                        if !bmp.is_null(){
                            let old=SelectObject(mem,bmp);
                            let mut g=null_mut();
                            if GdipCreateFromHDC(mem,&mut g)==0 {
                                if let Some(b)=GLOBAL.get(){overlay_frame(g,mem,w,h,b);}
                                GdipDeleteGraphics(g);
                            }
                            SetBkMode(mem,TRANSPARENT);
                            let face=wide("Segoe UI");
                            let font=CreateFontW(-13,0,0,0,650,0,0,0,1,0,0,5,0,face.as_ptr());
                            let previous=if !font.is_null(){Some(SelectObject(mem,font))}else{None};
                            LABELS.with(|labels|for label in labels.borrow().iter(){
                                let text=wide(&label.text);let len=(text.len()-1) as i32;
                                let mut extent=Point::default();GetTextExtentPoint32W(mem,text.as_ptr(),len,&mut extent);
                                let x=(label.x-if label.center{extent.x/2}else{0}).clamp(3,(w-extent.x-3).max(3));
                                let y=label.y.clamp(35,(h-extent.y-2).max(35));
                                SetTextColor(mem,0x00101820);
                                for (dx,dy) in [(-1,-1),(1,-1),(-1,1),(1,1)]{TextOutW(mem,x+dx,y+dy,text.as_ptr(),len);}
                                let color=((label.color&0xff)<<16)|(label.color&0xff00)|((label.color>>16)&0xff);
                                SetTextColor(mem,color);TextOutW(mem,x,y,text.as_ptr(),len);
                            });
                            if let Some(old)=previous{SelectObject(mem,old);DeleteObject(font);}
                            SetTextColor(mem,0x00c9e5ee);
                            STATUS.with(|s|{
                                // Keep long map / observer names clear of the close button.
                                let mut title=s.borrow().clone();
                                loop {
                                    let t=wide(&title);let mut size=Point::default();
                                    GetTextExtentPoint32W(mem,t.as_ptr(),(t.len()-1) as i32,&mut size);
                                    if size.x<=(w-65).max(0)||title.is_empty(){
                                        if t.len()>1{TextOutW(mem,12,8,t.as_ptr(),(t.len()-1) as i32);}break;
                                    }
                                    title.pop();
                                }
                            });
                            BOMB_LINES.with(|data|{
                                if let Some(lines)=data.borrow().as_ref(){
                                    let face=wide("Segoe UI");
                                    let face_bold=CreateFontW(-21,0,0,0,700,0,0,0,1,0,0,5,0,face.as_ptr());
                                    let face_small=CreateFontW(-16,0,0,0,500,0,0,0,1,0,0,5,0,face.as_ptr());
                                    SetTextColor(mem,0x00dbe8f4);
                                    if !face_small.is_null(){
                                        let old=SelectObject(mem,face_small);
                                        let t=wide(&lines[0]);TextOutW(mem,95,57,t.as_ptr(),(t.len()-1) as i32);
                                        SelectObject(mem,old);DeleteObject(face_small);
                                    }
                                    if !face_bold.is_null(){
                                        let old=SelectObject(mem,face_bold);
                                        SetTextColor(mem,0x00f7d5a1);
                                        let t=wide(&lines[1]);TextOutW(mem,95,82,t.as_ptr(),(t.len()-1) as i32);
                                        SelectObject(mem,old);DeleteObject(face_bold);
                                    }
                                    let center=wide("C4");
                                    TextOutW(mem,36,72,center.as_ptr(),(center.len()-1) as i32);
                                    SetTextColor(mem,0x00c9e5ee);
                                }
                            });
                            // Present completed frame in a single GDI blit.
                            BitBlt(screen,0,0,w,h,mem,0,0,SRCCOPY);
                            SelectObject(mem,old);
                            DeleteObject(bmp);
                        }
                        DeleteDC(mem);
                    }
                }
                EndPaint(hwnd,&paint);0
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
        let hwnd=CreateWindowExW(WS_EX_TOPMOST|WS_EX_LAYERED,class.as_ptr(),title.as_ptr(),WS_POPUP|WS_THICKFRAME,100,100,370,370,null_mut(),null_mut(),instance,null());
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
    #[test]fn defaults(){let s=OverlaySettings::default();assert!(s.topmost&&s.center_local&&s.follow_spectator&&!s.visible);assert!((s.zoom-1.0).abs()<0.01);}
    #[test]fn observer_setting_roundtrip(){
        let mut s=OverlaySettings::default();assert!(s.follow_spectator);
        assert!(s.update(&json!({"follow_spectator":false})).is_ok());
        assert!(!s.follow_spectator);
        assert!(s.update(&json!({"follow_spectator":"yes"})).is_err());
    }
    #[test]fn validate(){let mut s=OverlaySettings::default();assert!(s.update(&json!({"zoom":2.0,"visible":true,"self_color":"#00ffbb"})).is_ok());assert!(s.visible);assert!((s.zoom-2.).abs()<0.01);assert!(s.update(&json!({"zoom":20})).is_err());assert!(s.update(&json!({"self_color":"red"})).is_err());}
    #[test]fn timer_toggle(){let mut s=OverlaySettings::default();assert!(s.show_bomb_timer);
        assert!(s.update(&json!({"show_bomb_timer":false})).is_ok());
        assert!(!s.show_bomb_timer);
        assert!(s.update(&json!({"show_bomb_timer":"no"})).is_err());}
    #[test]fn site_toggle(){let mut s=OverlaySettings::default();assert!(s.show_sites);assert!(s.update(&json!({"show_sites":false})).is_ok());assert!(!s.show_sites);assert!(s.update(&json!({"show_sites":"yes"})).is_err());}
    #[test]fn follow_centers_me(){let map=(-3230.,1713.,5.);let me=[-1500.,500.,5.];
        let p=screen_point(me,map,360.,500.,1.6,Some(me)).unwrap();
        assert!((p.0-180.).abs()<0.001 && (p.1-250.).abs()<0.001);
        let other=screen_point([-1000.,500.,0.],map,360.,500.,1.6,Some(me)).unwrap();
        assert!(other.0>180.);
    }
    #[test]fn edge_marker_preserves_direction_after_rotation(){
        let (x,y,edge)=marker_point((2000.,180.),360.,360.,0.).unwrap();
        assert!(edge);assert_eq!(x,330.);assert_eq!(y,180.);
        let (x,y,edge)=marker_point((2000.,180.),360.,360.,90.).unwrap();
        assert!(edge);assert!((x-180.).abs()<0.01);assert!((y-298.).abs()<0.01);
        assert_eq!(marker_point((180.,180.),360.,360.,45.),Some((180.,180.,false)));
        assert!(marker_point((f32::NAN,0.),360.,360.,0.).is_none());
    }
    #[test]fn wide_zoom_is_supported(){
        let mut settings=OverlaySettings::default();
        assert!(settings.update(&json!({"zoom":0.35})).is_ok());
        assert!(settings.update(&json!({"zoom":0.34})).is_err());
        let near=screen_point([3000.,0.,0.],(0.,0.,5.),360.,360.,1.,Some([0.,0.,0.])).unwrap();
        let wide=screen_point([3000.,0.,0.],(0.,0.,5.),360.,360.,0.35,Some([0.,0.,0.])).unwrap();
        assert!((wide.0-180.).abs()<(near.0-180.).abs());
    }
    #[test]fn invalid_projection(){assert!(screen_point([0.,0.,0.],(0.,0.,0.),320.,320.,1.,None).is_none());}
}