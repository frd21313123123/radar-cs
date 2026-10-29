//! Stable per-install browser links. The access token remains a local secret.
use rand::{rngs::OsRng,RngCore};
use std::{fs,io::Write,path::Path};

pub fn random_token(bytes:usize)->String {
    let mut value=vec![0;bytes];OsRng.fill_bytes(&mut value);
    value.iter().map(|b|format!("{b:02x}")).collect()
}
pub fn read_access_token(cache:&Path)->Option<String>{
    let value=fs::read_to_string(cache.join("access_token.txt")).ok()?;
    let value=value.trim();
    (value.len()==48&&value.bytes().all(|b|b.is_ascii_hexdigit())).then(||value.to_owned())
}
pub fn access_token(cache:&Path)->Result<String,String>{
    if let Some(value)=read_access_token(cache){return Ok(value);}
    let value=random_token(24);
    let mut options=fs::OpenOptions::new();options.write(true).create(true).truncate(true);
    #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
    let mut file=options.open(cache.join("access_token.txt")).map_err(|e|format!("Не удалось сохранить ссылку радара: {e}"))?;
    file.write_all(value.as_bytes()).map_err(|e|e.to_string())?;
    Ok(value)
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn access_links_survive_restarts_and_invalid_cache_is_replaced(){
        let cache=std::env::temp_dir().join(format!("cas-session-{}",random_token(8)));
        fs::create_dir(&cache).unwrap();
        let first=access_token(&cache).unwrap();let second=access_token(&cache).unwrap();
        assert_eq!(first.len(),48);assert!(first==second);
        fs::write(cache.join("access_token.txt"),"invalid old token").unwrap();
        let replacement=access_token(&cache).unwrap();assert!(replacement!=first);
        assert!(read_access_token(&cache).is_some());
        fs::remove_dir_all(cache).unwrap();
    }
}
