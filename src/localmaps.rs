//! Discover maps directly from the installed CS2 files.
//!
//! `pak01_dir.vpk` contains the plain-text overview metadata and the compiled
//! radar textures. This module reads only the directory tree at startup, then
//! decodes a radar texture when the map is actually selected. A manual
//! calibration store (`cache/maps.local.json`) overrides both sources.
use crate::{
    gsi,
    model::normalize_map_name,
    vpk::VpkDir,
    vtex,
};
use serde_json::{json, Map, Value};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

const OVERVIEW_PREFIX: &str = "resource/overviews/";
const RADAR_PREFIX: &str = "panorama/images/overheadmaps/";

pub fn csgo_dir() -> Option<PathBuf> {
    for cfg in gsi::candidate_cfg_dirs() {
        if let Some(csgo) = cfg.parent() {
            if csgo.join("pak01_dir.vpk").is_file() {
                return Some(csgo.to_owned());
            }
        }
    }
    None
}

fn cache_stamp(path: &Path) -> String {
    let Ok(meta) = fs::metadata(path) else { return String::new(); };
    let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs()).unwrap_or(0);
    format!("{}:{}", meta.len(), modified)
}

#[derive(Debug)]
struct Token {
    value: String,
}

fn tokenize(text: &str) -> Vec<Token> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
            }
            b'{' => { tokens.push(Token { value: "{".into() }); i += 1; }
            b'}' => { tokens.push(Token { value: "}".into() }); i += 1; }
            b'"' => {
                i += 1;
                let mut value = String::new();
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 1;
                    }
                    value.push(bytes[i] as char);
                    i += 1;
                }
                i = (i + 1).min(bytes.len());
                tokens.push(Token { value });
            }
            _ => {
                let start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace()
                    && bytes[i] != b'{' && bytes[i] != b'}' && bytes[i] != b'"' {
                    i += 1;
                }
                tokens.push(Token { value: text[start..i].to_owned() });
            }
        }
    }
    tokens
}

fn scalar(value: &str) -> Value {
    if let Ok(number) = value.parse::<f64>() {
        if number.is_finite() {
            return json!(number);
        }
    }
    json!(value)
}

fn parse_object(tokens: &[Token], at: &mut usize) -> Option<Value> {
    if tokens.get(*at)?.value != "{" { return None; }
    *at += 1;
    let mut object = Map::new();
    while *at < tokens.len() && tokens[*at].value != "}" {
        let key = tokens[*at].value.clone();
        *at += 1;
        let value = if tokens.get(*at)?.value == "{" {
            parse_object(tokens, at)?
        } else {
            let value = tokens.get(*at)?.value.clone();
            *at += 1;
            scalar(&value)
        };
        object.insert(key, value);
    }
    if tokens.get(*at)?.value != "}" { return None; }
    *at += 1;
    Some(Value::Object(object))
}

/// Parse a CS2 `resource/overviews/*.txt` file. The first quoted token is the
/// map name; the following block contains the projection metadata.
fn parse_overview(text: &str) -> Option<Value> {
    let tokens = tokenize(text);
    let mut at = 0;
    if tokens.get(0)?.value == "{" {
        return parse_object(&tokens, &mut at);
    }
    let _name = tokens.get(0)?;
    at = 1;
    parse_object(&tokens, &mut at)
}

fn title_from_name(name: &str) -> String {
    let base = name.split_once('_').map(|(_, rest)| rest).unwrap_or(name);
    base.split(['_', '-']).filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn display_name(name: &str) -> String {
    match name {
        "de_cbble" => "Cobblestone".into(),
        "de_dust2" => "Dust II".into(),
        "de_mirage" => "Mirage".into(),
        "de_inferno" => "Inferno".into(),
        "de_ancient" => "Ancient".into(),
        "de_anubis" => "Anubis".into(),
        "de_nuke" => "Nuke".into(),
        "de_train" => "Train".into(),
        "de_vertigo" => "Vertigo".into(),
        "de_overpass" => "Overpass".into(),
        "de_cache" => "Cache".into(),
        _ => title_from_name(name),
    }
}

fn radar_file(map: &str, layer: &str, ext: &str) -> String {
    if layer == "default" {
        format!("{map}_radar_{ext}.vtex_c")
    } else {
        format!("{map}_{layer}_radar_{ext}.vtex_c")
    }
}

fn radar_path(map: &str, layer: &str, ext: &str) -> String {
    format!("{RADAR_PREFIX}{}", radar_file(map, layer, ext))
}

fn local_file(map: &str, layer: &str, ext: &str) -> String {
    if layer == "default" {
        format!("{map}_radar_{ext}.png")
    } else {
        format!("{map}_{layer}_radar_{ext}.png")
    }
}

fn safe_file(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 180
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        && !name.starts_with('.')
        && (name.ends_with(".png") || name.ends_with(".png.part"))
}

fn local_map_entry(map: &str, overview: &Value, radar_index: &HashMap<String, String>) -> Option<Value> {
    let object = overview.as_object()?;
    let pos_x = object.get("pos_x")?.as_f64()?;
    let pos_y = object.get("pos_y")?.as_f64()?;
    let scale = object.get("scale")?.as_f64()?;
    if !(pos_x.is_finite() && pos_y.is_finite() && scale.is_finite() && scale > 0.) {
        return None;
    }
    let layers = object.get("verticalsections").cloned().unwrap_or_else(|| json!({}));
    let mut layer_names = vec!["default".to_string()];
    if let Some(map) = layers.as_object() {
        for (name, bounds) in map {
            if name.len() <= 20 && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
                && bounds.as_object().is_some_and(|b| b.contains_key("AltitudeMin") && b.contains_key("AltitudeMax")) {
                layer_names.push(name.clone());
            }
        }
    }
    let mut game_radars = Map::new();
    let mut radar_paths = Vec::new();
    for layer in &layer_names {
        let file = ["psd", "tga"].into_iter().find_map(|ext| {
            let path = radar_path(&map, layer, ext);
            radar_index.contains_key(&path).then(|| (ext, path))
        });
        let Some((ext, path)) = file else { continue; };
        let cache_name = local_file(map, layer, ext);
        game_radars.insert(layer.clone(), json!(path));
        radar_paths.push(json!(format!("local:{cache_name}")));
    }
    if radar_paths.is_empty() {
        return None;
    }
    let mut entry = Map::new();
    entry.insert("display_name".into(), json!(display_name(map)));
    entry.insert("source".into(), json!("game"));
    entry.insert("radar_paths".into(), Value::Array(radar_paths));
    entry.insert("game_radars".into(), Value::Object(game_radars));
    let mut info = Map::new();
    info.insert("pos_x".into(), json!(pos_x));
    info.insert("pos_y".into(), json!(pos_y));
    info.insert("scale".into(), json!(scale));
    for key in ["rotate", "zoom"] {
        if let Some(value) = object.get(key) {
            info.insert(key.into(), value.clone());
        }
    }
    if layers.as_object().is_some_and(|m| !m.is_empty()) {
        info.insert("verticalsections".into(), layers);
    }
    entry.insert("radar_info".into(), Value::Object(info));
    if let (Some(a), Some(b)) = (object.get("bombA_x"), object.get("bombA_y")) {
        let (Some(ax), Some(ay)) = (a.as_f64(), b.as_f64()) else { return None; };
        if let (Some(bx), Some(by)) = (object.get("bombB_x"), object.get("bombB_y")) {
            if let (Some(bx), Some(by)) = (bx.as_f64(), by.as_f64()) {
                entry.insert("bombsites".into(), json!({
                    "A": {"x": ax, "y": ay}, "B": {"x": bx, "y": by}
                }));
            }
        }
    }
    Some(Value::Object(entry))
}

fn discover_from_vpk(csgo: &Path) -> Result<Value, String> {
    let vpk_path = csgo.join("pak01_dir.vpk");
    let vpk = VpkDir::open(&vpk_path)?;
    let mut radar_index = HashMap::new();
    for path in vpk.paths() {
        if path.starts_with(RADAR_PREFIX) && path.ends_with(".vtex_c") {
            if let Some(file) = path.rsplit('/').next() {
                radar_index.insert(path.clone(), file.to_owned());
            }
        }
    }
    let mut maps = Map::new();
    for path in vpk.paths() {
        if !path.starts_with(OVERVIEW_PREFIX) || !path.ends_with(".txt") {
            continue;
        }
        let Some(file) = path.rsplit('/').next() else { continue; };
        let Some(raw_name) = file.strip_suffix(".txt") else { continue; };
        let Some(name) = normalize_map_name(raw_name) else { continue; };
        let Ok(bytes) = vpk.extract(path) else { continue; };
        let Ok(text) = std::str::from_utf8(&bytes) else { continue; };
        let Some(overview) = parse_overview(text) else { continue; };
        if let Some(entry) = local_map_entry(&name, &overview, &radar_index) {
            maps.insert(name, entry);
        }
    }
    Ok(json!({"maps": Value::Object(maps)}))
}

fn cached_maps(cache: &Path) -> Option<Value> {
    let path = cache.join("game_maps.json");
    let value: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    value.get("maps").cloned()
}

/// Load game maps, using `cache/game_maps.json` while the game VPK is unchanged.
pub fn load_game_maps(cache: &Path) -> Value {
    let Some(csgo) = csgo_dir() else { return json!({"maps":{}}); };
    let vpk_path = csgo.join("pak01_dir.vpk");
    let stamp = cache_stamp(&vpk_path);
    let cache_path = cache.join("game_maps.json");
    if let Some(value) = fs::read(&cache_path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
        if value.get("stamp").and_then(Value::as_str) == Some(stamp.as_str()) {
            if let Some(maps) = value.get("maps") { return json!({"maps": maps}); }
        }
    }
    match discover_from_vpk(&csgo) {
        Ok(result) => {
            let _ = fs::create_dir_all(cache);
            let _ = fs::write(&cache_path, serde_json::to_vec(&json!({"stamp": stamp, "maps": result["maps"]})).unwrap_or_default());
            result
        }
        Err(error) => {
            eprintln!("[maps] CS2 map discovery failed: {error}");
            cached_maps(cache).map(|maps| json!({"maps": maps})).unwrap_or_else(|| json!({"maps":{}}))
        }
    }
}

pub fn load_calibrations(cache: &Path) -> Value {
    fs::read(cache.join("maps.local.json")).ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v.get("maps").cloned())
        .map(|maps| json!({"maps": maps}))
        .unwrap_or_else(|| json!({"maps":{}}))
}

pub fn fill_missing(target: &mut Value, extra: &Value, overwrite: bool) -> usize {
    let Some(extra_maps) = extra.get("maps").and_then(Value::as_object) else { return 0; };
    let Some(target_maps) = target.get_mut("maps").and_then(Value::as_object_mut) else { return 0; };
    let mut added = 0;
    for (name, entry) in extra_maps {
        let usable = entry.get("radar_info").and_then(|v| v.get("scale")).and_then(Value::as_f64)
            .is_some_and(|v| v.is_finite() && v > 0.);
        if !usable { continue; }
        let replace = overwrite || match target_maps.get(name) {
            None => true,
            Some(current) => current.get("radar_info").and_then(|v| v.get("scale")).and_then(Value::as_f64)
                .is_none_or(|v| !v.is_finite() || v <= 0.),
        };
        if replace {
            target_maps.insert(name.clone(), entry.clone());
            added += 1;
        }
    }
    added
}

fn local_image_dir(cache: &Path) -> PathBuf { cache.join("local_maps") }

pub fn local_image(cache: &Path, file: &str) -> Result<Vec<u8>, String> {
    if !safe_file(file) { return Err("Invalid local map file".into()); }
    fs::read(local_image_dir(cache).join(file)).map_err(|e| format!("Local map image: {e}"))
}

/// Return a local radar image, decoding it from the game VPK on first use.
pub fn materialize(cache: &Path, map: &str, layer: &str, entry: &Value) -> Result<Vec<u8>, String> {
    let path = entry.get("game_radars").and_then(|v| v.get(layer)).and_then(Value::as_str)
        .ok_or_else(|| "No game radar for this layer".to_string())?;
    let ext = if path.contains("_radar_tga.") { "tga" } else { "psd" };
    let file = local_file(map, layer, ext);
    let target = local_image_dir(cache).join(&file);
    if let Ok(bytes) = fs::read(&target) {
        if crate::assets::valid_map_png(&bytes) { return Ok(bytes); }
    }
    let csgo = csgo_dir().ok_or("CS2 installation with pak01_dir.vpk was not found")?;
    let vpk = VpkDir::open(&csgo.join("pak01_dir.vpk"))?;
    let source = vpk.extract(path)?;
    let png = vtex::decode_radar_png(&source)?;
    fs::create_dir_all(local_image_dir(cache)).map_err(|e| e.to_string())?;
    let temporary = target.with_extension("png.part");
    fs::write(&temporary, &png).map_err(|e| e.to_string())?;
    if fs::rename(&temporary, &target).is_err() {
        let _ = fs::remove_file(&target);
        fs::rename(&temporary, &target).map_err(|e| e.to_string())?;
    }
    Ok(png)
}

pub fn save_calibration_image(cache: &Path, map: &str, png: &[u8]) -> Result<String, String> {
    let name = normalize_map_name(map).ok_or("Invalid map name")?;
    if !crate::assets::valid_map_png(png) { return Err("Файл не является корректным PNG".into()); }
    fs::create_dir_all(local_image_dir(cache)).map_err(|e| e.to_string())?;
    let file = format!("{name}_radar_psd.png");
    fs::write(local_image_dir(cache).join(&file), png).map_err(|e| e.to_string())?;
    Ok(file)
}

pub fn save_calibration(cache: &Path, request: &Value, image_file: &str) -> Result<Value, String> {
    let name = request.get("name").and_then(Value::as_str).and_then(normalize_map_name)
        .ok_or("Некорректное имя карты")?;
    let display = request.get("display_name").and_then(Value::as_str)
        .filter(|v| !v.is_empty() && v.len() <= 80)
        .map(str::to_owned).unwrap_or_else(|| display_name(&name));
    let pos_x = request.get("pos_x").and_then(Value::as_f64).ok_or("pos_x отсутствует")?;
    let pos_y = request.get("pos_y").and_then(Value::as_f64).ok_or("pos_y отсутствует")?;
    let scale = request.get("scale").and_then(Value::as_f64).ok_or("scale отсутствует")?;
    if !(pos_x.is_finite() && pos_y.is_finite() && scale.is_finite() && scale > 0. && scale < 1000.) {
        return Err("Некорректные координаты или масштаб карты".into());
    }
    let mut entry = Map::new();
    entry.insert("display_name".into(), json!(display));
    entry.insert("source".into(), json!("calibration"));
    entry.insert("radar_paths".into(), json!([format!("local:{image_file}")]));
    entry.insert("radar_info".into(), json!({"pos_x": pos_x, "pos_y": pos_y, "scale": scale}));
    if let Some(sites) = request.get("bombsites") {
        let a = sites.get("A").and_then(|v| Some((v.get("x")?.as_f64()?, v.get("y")?.as_f64()?)));
        let b = sites.get("B").and_then(|v| Some((v.get("x")?.as_f64()?, v.get("y")?.as_f64()?)));
        if let (Some(a), Some(b)) = (a, b) {
            if [a.0, a.1, b.0, b.1].iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)) {
                entry.insert("bombsites".into(), json!({"A": {"x": a.0, "y": a.1}, "B": {"x": b.0, "y": b.1}}));
            }
        }
    }
    let path = cache.join("maps.local.json");
    let mut data = fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .unwrap_or_else(|| json!({"maps":{}}));
    if !data.get("maps").is_some_and(Value::is_object) { data = json!({"maps":{}}); }
    data["maps"][&name] = Value::Object(entry.clone());
    let bytes = serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("json.part");
    fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
    fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
    Ok(Value::Object(entry))
}

pub fn remove_calibration(cache: &Path, map: &str) -> Result<(), String> {
    let name = normalize_map_name(map).ok_or("Invalid map name")?;
    let path = cache.join("maps.local.json");
    let mut data = fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .unwrap_or_else(|| json!({"maps":{}}));
    if let Some(maps) = data.get_mut("maps").and_then(Value::as_object_mut) {
        maps.remove(&name);
    }
    let bytes = serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?;
    fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let _ = fs::remove_file(local_image_dir(cache).join(format!("{name}_radar_psd.png")));
    let _ = fs::remove_file(local_image_dir(cache).join(format!("{name}_radar.png")));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_overview_with_layers_and_sites() {
        let text = r#""de_test"
{
    "pos_x" "-100"
    "pos_y" "200"
    "scale" "4.5"
    "bombA_x" "0.25"
    "bombA_y" "0.75"
    "bombB_x" "0.6"
    "bombB_y" "0.4"
    "verticalsections"
    {
        "default" { "AltitudeMax" "100" "AltitudeMin" "-10" }
        "lower" { "AltitudeMax" "-10" "AltitudeMin" "-100" }
    }
}"#;
        let value = parse_overview(text).unwrap();
        assert_eq!(value["pos_x"].as_f64(), Some(-100.));
        assert_eq!(value["scale"].as_f64(), Some(4.5));
        assert_eq!(value["verticalsections"]["lower"]["AltitudeMin"].as_f64(), Some(-100.));
        let index = HashMap::new();
        assert!(local_map_entry("de_test", &value, &index).is_none(), "no radar image -> no map");
    }

    #[test]
    fn safe_local_names() {
        assert!(safe_file("de_test_radar_psd.png"));
        assert!(!safe_file("../de_test.png"));
        assert!(!safe_file("de_test.png.exe"));
    }

    #[test]
    fn decodes_dxt5_radar_from_the_game() {
        let Some(csgo)=csgo_dir()else{return;};
        let Ok(vpk)=VpkDir::open(&csgo.join("pak01_dir.vpk"))else{return;};
        let bytes=vpk.extract("panorama/images/overheadmaps/ar_shoots_radar_psd.vtex_c").unwrap();
        let png=vtex::decode_radar_png(&bytes).unwrap();
        assert!(crate::assets::valid_map_png(&png));
    }

    #[test]
    fn extracts_real_radar_when_game_is_installed() {
        if csgo_dir().is_none() { return; }
        let cache = std::env::temp_dir().join(format!("cas-localmaps-real-{}", std::process::id()));
        let _ = fs::remove_dir_all(&cache);
        fs::create_dir_all(&cache).unwrap();
        let maps = load_game_maps(&cache);
        let entry = &maps["maps"]["de_cbble"];
        assert_eq!(entry["radar_info"]["scale"].as_f64(), Some(4.724792));
        let png = materialize(&cache, "de_cbble", "default", entry).unwrap();
        assert!(crate::assets::valid_map_png(&png));
        let _ = fs::remove_dir_all(cache);
    }

    #[test]
    fn fill_missing_keeps_existing_remote() {
        let mut target = json!({"maps":{"de_dust2":{"radar_info":{"scale":4.4}}}});
        let extra = json!({"maps":{
            "de_dust2":{"radar_info":{"scale":9.0}},
            "de_new":{"radar_info":{"scale":5.0}},
            "broken":{"radar_info":{}}
        }});
        assert_eq!(fill_missing(&mut target, &extra, false), 1);
        assert_eq!(target["maps"]["de_dust2"]["radar_info"]["scale"], 4.4);
        assert_eq!(target["maps"]["de_new"]["radar_info"]["scale"], 5.0);
        assert!(target["maps"].get("broken").is_none());
    }
}
