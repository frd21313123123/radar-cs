use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone,Debug)]
pub struct Schema {
    offsets: HashMap<String,u64>,
    engine: HashMap<String,u64>,
    classes: Value,
    pub info: Value,
}
impl Schema {
    pub fn from_json(offsets:&Value, client:&Value, info:Value) -> Result<Self,String> {
        let scan = |v:&Value| -> HashMap<String,u64> {
            v.as_object().map(|m| m.iter().filter_map(|(k,v)|v.as_u64().map(|u|(k.clone(),u))).collect()).unwrap_or_default()
        };
        let s=Self { offsets:scan(&offsets["client.dll"]),engine:scan(&offsets["engine2.dll"]),classes:client["client.dll"]["classes"].clone(),info };
        for name in ["dwEntityList","dwLocalPlayerController"] {s.offset(name).ok_or(format!("missing {name}"))?;}
        for (class,field) in [("CCSPlayerController","m_hPlayerPawn"),("C_BaseEntity","m_iTeamNum"),
            ("C_BaseEntity","m_iHealth"),("C_BaseEntity","m_pGameSceneNode"),("CGameSceneNode","m_vecAbsOrigin")] {s.field(class,field).ok_or(format!("missing {class}.{field}"))?;}
        Ok(s)
    }
    pub fn offset(&self,name:&str)->Option<u64>{self.offsets.get(name).copied()}
    /// Overrides are process-local and only supplied by the verified PE resolver.
    pub(crate) fn recover_player_offsets(&mut self,entity_list:u64,local_controller:u64){
        self.offsets.insert("dwEntityList".into(),entity_list);
        self.offsets.insert("dwLocalPlayerController".into(),local_controller);
    }
    pub fn engine_offset(&self,name:&str)->Option<u64>{self.engine.get(name).copied()}
    pub fn field(&self, class:&str, name:&str)->Option<u64>{
        let mut class=class.to_owned();
        for _ in 0..12 {
            let obj=self.classes.get(&class)?;
            if let Some(n)=obj.get("fields").and_then(|v|v.get(name)).and_then(Value::as_u64){return Some(n);}
            class=obj.get("parent")?.as_str()?.to_owned();
        }
        None
    }
}