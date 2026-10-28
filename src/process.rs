//! Windows process access: VM_READ + QUERY_INFORMATION only. No injection or writes.
use std::collections::HashMap;

pub trait Memory {
    fn read(&mut self, addr:u64, len:usize)->Option<Vec<u8>>;
    fn read_count(&self)->u64 {0}
    fn fail_count(&self)->u64 {0}
    fn last_error(&self)->u32 {0}
    fn u8(&mut self,addr:u64)->u8{self.read(addr,1).and_then(|v|v.first().copied()).unwrap_or(0)}
    fn u16(&mut self,addr:u64)->u16{self.read(addr,2).and_then(|v|v.try_into().ok()).map(u16::from_le_bytes).unwrap_or(0)}
    fn u32(&mut self,addr:u64)->u32{self.read(addr,4).and_then(|v|v.try_into().ok()).map(u32::from_le_bytes).unwrap_or(0)}
    fn i32(&mut self,addr:u64)->i32{self.read(addr,4).and_then(|v|v.try_into().ok()).map(i32::from_le_bytes).unwrap_or(0)}
    fn u64(&mut self,addr:u64)->u64{self.read(addr,8).and_then(|v|v.try_into().ok()).map(u64::from_le_bytes).unwrap_or(0)}
    fn vec3(&mut self,addr:u64)->Option<[f32;3]>{
        let raw=self.read(addr,12)?;
        let a=[f32::from_le_bytes(raw[0..4].try_into().ok()?),f32::from_le_bytes(raw[4..8].try_into().ok()?),f32::from_le_bytes(raw[8..12].try_into().ok()?)];
        if a.iter().all(|v| v.is_finite()&&v.abs()<100000.){Some(a)}else{None}
    }
    fn cstr(&mut self,addr:u64,len:usize)->String{
        self.read(addr,len).map(|v|{
            let end=v.iter().position(|x|*x==0).unwrap_or(v.len());
            String::from_utf8_lossy(&v[..end]).trim().to_string()
        }).unwrap_or_default()
    }
}

#[cfg(windows)] mod native {
    use super::*;
    use std::{mem,ptr};
    use windows_sys::Win32::Foundation::{CloseHandle,GetLastError,INVALID_HANDLE_VALUE,HANDLE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot,Process32FirstW,Process32NextW,
        Module32FirstW,Module32NextW,PROCESSENTRY32W,MODULEENTRY32W,TH32CS_SNAPPROCESS,TH32CS_SNAPMODULE,TH32CS_SNAPMODULE32};
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::{OpenProcess,PROCESS_VM_READ,PROCESS_QUERY_INFORMATION};

    fn wide(s:&[u16])->String {let n=s.iter().position(|x|*x==0).unwrap_or(s.len());String::from_utf16_lossy(&s[..n])}
    pub struct WinProcess { handle:HANDLE, pub pid:u32,pub modules:HashMap<String,u64>,reads:u64,fails:u64,error:u32 }
    // WinProcess is only used in its own polling thread; HANDLE isn't shared.
    impl WinProcess {
        pub fn new()->Self{Self{handle:ptr::null_mut(),pid:0,modules:HashMap::new(),reads:0,fails:0,error:0}}
        pub fn close(&mut self){unsafe{if !self.handle.is_null(){CloseHandle(self.handle);}}self.handle=ptr::null_mut();self.pid=0;self.modules.clear();}
        fn find_pid()->Result<Option<u32>,String>{unsafe{
            let snap=CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS,0);
            if snap==INVALID_HANDLE_VALUE{return Err("Process snapshot failed".into());}
            let mut entry:PROCESSENTRY32W=mem::zeroed();entry.dwSize=mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found=None;
            if Process32FirstW(snap,&mut entry)!=0 {loop{
                if wide(&entry.szExeFile).eq_ignore_ascii_case("cs2.exe"){found=Some(entry.th32ProcessID);break;}
                if Process32NextW(snap,&mut entry)==0 {break;}
            }}
            CloseHandle(snap);Ok(found)
        }}
        fn enumerate_modules(pid:u32)->Result<HashMap<String,u64>,String>{unsafe{
            let snap=CreateToolhelp32Snapshot(TH32CS_SNAPMODULE|TH32CS_SNAPMODULE32,pid);
            if snap==INVALID_HANDLE_VALUE{return Err(format!("Module snapshot failed: {}",GetLastError()));}
            let mut entry:MODULEENTRY32W=mem::zeroed();entry.dwSize=mem::size_of::<MODULEENTRY32W>() as u32;
            let mut out=HashMap::new();
            if Module32FirstW(snap,&mut entry)!=0 {loop{
                out.insert(wide(&entry.szModule).to_ascii_lowercase(),entry.modBaseAddr as u64);
                if Module32NextW(snap,&mut entry)==0{break;}
            }}
            CloseHandle(snap);Ok(out)
        }}
        pub fn connect(&mut self)->Result<bool,String>{
            let pid=match Self::find_pid()? {Some(id)=>id,None=>{self.close();return Ok(false);}};
            if pid==self.pid && !self.handle.is_null(){return Ok(true);}
            self.close();
            let h=unsafe{OpenProcess(PROCESS_VM_READ|PROCESS_QUERY_INFORMATION,0,pid)};
            if h.is_null(){return Err(format!("OpenProcess denied: {}",unsafe{GetLastError()}));}
            self.handle=h;self.pid=pid;
            self.modules=Self::enumerate_modules(pid)?;
            if !self.modules.contains_key("client.dll"){return Err("client.dll not loaded".into());}
            Ok(true)
        }
        pub fn module(&self,name:&str)->u64{self.modules.get(name).copied().unwrap_or(0)}
    }
    impl Drop for WinProcess{fn drop(&mut self){self.close();}}
    impl Memory for WinProcess{
        fn read(&mut self, addr:u64,len:usize)->Option<Vec<u8>>{
            self.reads+=1;
            if self.handle.is_null()||addr<0x10000||addr>0x7fff_ffff_ffff||len==0||len>16*1024 {self.fails+=1;return None;}
            let mut data=vec![0_u8;len];let mut read=0usize;
            let ok=unsafe{ReadProcessMemory(self.handle,addr as *const _,data.as_mut_ptr() as *mut _,len,&mut read)};
            if ok==0||read!=len {self.fails+=1;self.error=if ok==0 {unsafe{GetLastError()}}else{299};None}else{Some(data)}
        }
        fn read_count(&self)->u64{self.reads}
        fn fail_count(&self)->u64{self.fails}
        fn last_error(&self)->u32{self.error}
    }
}
#[cfg(windows)] pub use native::WinProcess;

#[cfg(not(windows))] pub struct WinProcess{pub pid:u32,pub modules:HashMap<String,u64>}
#[cfg(not(windows))] impl WinProcess {
    pub fn new()->Self{Self{pid:0,modules:HashMap::new()}}
    pub fn connect(&mut self)->Result<bool,String>{Err("Live mode requires Windows 64-bit".into())}
    pub fn close(&mut self){}
    pub fn module(&self,_name:&str)->u64{0}
}
#[cfg(not(windows))] impl Memory for WinProcess{fn read(&mut self,_addr:u64,_len:usize)->Option<Vec<u8>>{None}}

#[cfg(test)]
pub struct FakeMemory{pub mem:HashMap<u64,u8>}
#[cfg(test)]impl FakeMemory{pub fn new()->Self{Self{mem:HashMap::new()}}pub fn put(&mut self,addr:u64,b:&[u8]){for (i,v) in b.iter().enumerate(){self.mem.insert(addr+i as u64,*v);}}}
#[cfg(test)]impl Memory for FakeMemory{fn read(&mut self,addr:u64,len:usize)->Option<Vec<u8>>{(0..len).map(|i|self.mem.get(&(addr+i as u64)).copied()).collect()}}