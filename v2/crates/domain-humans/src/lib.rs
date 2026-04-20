//! Domain-humans — porta das funções de humans em `plugin/ui/server.cjs`.
//!
//! Humans representam os usuários reais (pessoas físicas) que conversam
//! com os agentes. Persistidos em `<memDir>/humans.json` + log por humano
//! em `<memDir>/human_logs/<id>.jsonl`.
//!
//! # Escopo desta crate
//! - Load/save do `humans.json` (sempre tolerante a JSON inválido).
//! - Sanitização de campos livres (location/gender/skin/position/ui_prefs).
//! - Slug de nome → id (NFD + toLowerCase + kebab + cap 40 chars).
//! - CRUD + heartbeat + append-only log.
//!
//! Chat entre humanos (`human_chats/`) e shadow-turns (Claude spawn)
//! ficam fora — pertencem a F4-05d-2 e F4-05f.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use thiserror::Error;

/// Janela (ms) para considerar um humano online após último heartbeat.
/// Paridade com `ONLINE_WINDOW_MS` no `.cjs` (60_000 ms).
pub const ONLINE_WINDOW_MS: i64 = 60_000;

/// Valores aceitos em `human.location`.
pub const HUMAN_LOCATIONS: &[&str] = &[
    "core",
    "devs",
    "specialists",
    "integrators",
    "motors",
    "firefighter",
];

/// Valores aceitos em `human.gender`.
pub const HUMAN_GENDERS: &[&str] = &["male", "female", "other"];

/// Valores aceitos em `human.skin.outfit`.
pub const HUMAN_OUTFITS: &[&str] = &["pants", "dress"];

#[derive(Debug, Error)]
pub enum Error {
    #[error("need name")]
    NeedName,
    #[error("not found")]
    NotFound,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Abstrai `new Date().toISOString() / Date.now()`. Em prod usamos
/// [`SystemClock`]; em testes [`FixedClock`].
pub trait Clock {
    fn now_iso(&self) -> String;
    fn now_ms(&self) -> i64;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_iso(&self) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        format_iso_millis(ms)
    }
    fn now_ms(&self) -> i64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
}

/// Clock fixo para testes. `ms_epoch` vira a base de `now_ms`; `now_iso`
/// formata essa mesma base.
#[derive(Debug, Clone, Copy)]
pub struct FixedClock(pub i64);

impl Clock for FixedClock {
    fn now_iso(&self) -> String {
        format_iso_millis(self.0)
    }
    fn now_ms(&self) -> i64 {
        self.0
    }
}

fn format_iso_millis(ms: i64) -> String {
    // Decomposição Gregoriana idêntica à usada em outros domain crates.
    let mut seconds = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let days = seconds.div_euclid(86_400);
    seconds = seconds.rem_euclid(86_400);
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{hours:02}:{minutes:02}:{secs:02}.{millis:03}Z"
    )
}

fn civil_from_days(mut z: i64) -> (i64, u32, u32) {
    z += 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Parseia ISO 8601 `YYYY-MM-DDTHH:MM:SS.sssZ` → epoch ms.
/// Tolerante: retorna `None` se mal-formado.
pub fn parse_iso_to_ms(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    if bytes.len() < 10 {
        return None;
    }
    let year: i64 = s.get(..4)?.parse().ok()?;
    let month: i64 = s.get(5..7)?.parse().ok()?;
    let day: i64 = s.get(8..10)?.parse().ok()?;
    let (hour, min, sec, ms) = if bytes.len() >= 19 {
        let h: i64 = s.get(11..13)?.parse().ok()?;
        let mi: i64 = s.get(14..16)?.parse().ok()?;
        let sec: i64 = s.get(17..19)?.parse().ok()?;
        let ms: i64 = if bytes.len() >= 23 && &bytes[19..20] == b"." {
            s.get(20..23).and_then(|x| x.parse().ok()).unwrap_or(0)
        } else {
            0
        };
        (h, mi, sec, ms)
    } else {
        (0, 0, 0, 0)
    };
    let days = days_from_civil(year, month, day);
    Some(days * 86_400_000 + hour * 3_600_000 + min * 60_000 + sec * 1_000 + ms)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// ── Paths ────────────────────────────────────────────────────────────────

/// Paridade com `humansFile = path.join(memDir, 'humans.json')`.
pub fn humans_file(mem_dir: &Path) -> PathBuf {
    mem_dir.join("humans.json")
}

/// Paridade com `humanLogDir = path.join(memDir, 'human_logs')`.
pub fn human_log_dir(mem_dir: &Path) -> PathBuf {
    mem_dir.join("human_logs")
}

pub fn human_log_path(mem_dir: &Path, id: &str) -> PathBuf {
    human_log_dir(mem_dir).join(format!("{id}.jsonl"))
}

// ── Slug ─────────────────────────────────────────────────────────────────

/// Paridade com `slugHuman` em `server.cjs:226`.
///
/// - NFD → remove combining marks (`\u0300-\u036f`)
/// - toLowerCase
/// - trim
/// - `[^a-z0-9]+` → `-`
/// - `^-+|-+$` → ``
/// - slice 0..40
/// - fallback: `human-{now_ms}`
pub fn slug_human(name: &str, now_ms: i64) -> String {
    // NFD + remoção de diacritics. Rust não tem NFD no `std`, fazemos manual
    // para o subset latim usado em PT/EN/ES/FR (ASCII + acentos compostos).
    let mut nfd = String::with_capacity(name.len());
    for c in name.chars() {
        if let Some(stripped) = strip_diacritic(c) {
            nfd.push(stripped);
        } else {
            nfd.push(c);
        }
    }
    let lower = nfd.to_lowercase();
    let trimmed = lower.trim();
    let mut slug = String::new();
    let mut prev_dash = false;
    for c in trimmed.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    while slug.starts_with('-') {
        slug.remove(0);
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    // slice 0..40 em UTF-16 code units (paridade com String.prototype.slice
    // do JS, que opera em UTF-16). Como o slug só tem ASCII aqui, isso
    // equivale a byte slice.
    if slug.len() > 40 {
        slug.truncate(40);
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        return format!("human-{now_ms}");
    }
    slug
}

/// Remove diacritic for common Latin characters (NFD decomposition subset).
fn strip_diacritic(c: char) -> Option<char> {
    Some(match c {
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'A',
        'È' | 'É' | 'Ê' | 'Ë' => 'E',
        'Ì' | 'Í' | 'Î' | 'Ï' => 'I',
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' => 'O',
        'Ù' | 'Ú' | 'Û' | 'Ü' => 'U',
        'Ý' | 'Ÿ' => 'Y',
        'Ç' => 'C',
        'Ñ' => 'N',
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        'ç' => 'c',
        'ñ' => 'n',
        _ => return None,
    })
}

// ── Sanitização de campos ────────────────────────────────────────────────

pub fn sanitize_location(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?;
    if HUMAN_LOCATIONS.contains(&s) {
        Some(s.to_string())
    } else {
        None
    }
}

pub fn sanitize_gender(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?;
    if HUMAN_GENDERS.contains(&s) {
        Some(s.to_string())
    } else {
        None
    }
}

/// Replica `sanitizeSkin` em `server.cjs:270`.
pub fn sanitize_skin(v: Option<&Value>) -> Option<Value> {
    let obj = v?.as_object()?;
    let num = |key: &str| -> Option<i64> {
        let n = obj.get(key)?.as_f64()?;
        if n.is_finite() && n >= 0.0 && n <= 0xff_ffff as f64 {
            // `| 0` no JS: truncate para i32. Preservamos o comportamento.
            Some(n as i64)
        } else {
            None
        }
    };
    let skin_tone = num("skinTone");
    let hair = num("hair");
    let shirt = num("shirt");
    let pants = num("pants");
    let outfit = obj
        .get("outfit")
        .and_then(Value::as_str)
        .filter(|s| HUMAN_OUTFITS.contains(s))
        .map(str::to_string);
    if skin_tone.is_none()
        && hair.is_none()
        && shirt.is_none()
        && pants.is_none()
        && outfit.is_none()
    {
        return None;
    }
    let mut m = Map::new();
    m.insert("skinTone".into(), Value::from(skin_tone.unwrap_or(0xf1_c27d)));
    m.insert("hair".into(), Value::from(hair.unwrap_or(0x44_403c)));
    m.insert("shirt".into(), Value::from(shirt.unwrap_or(0x3b_82f6)));
    m.insert("pants".into(), Value::from(pants.unwrap_or(0x1f_2937)));
    m.insert(
        "outfit".into(),
        Value::String(outfit.unwrap_or_else(|| "pants".to_string())),
    );
    Some(Value::Object(m))
}

pub fn sanitize_position(v: Option<&Value>) -> Option<Value> {
    let obj = v?.as_object()?;
    let x = obj.get("x").and_then(Value::as_f64).filter(|n| n.is_finite())?;
    let y = obj.get("y").and_then(Value::as_f64).filter(|n| n.is_finite())?;
    let mut m = Map::new();
    m.insert("x".into(), json!(x));
    m.insert("y".into(), json!(y));
    Some(Value::Object(m))
}

pub fn sanitize_ui_prefs(v: Option<&Value>) -> Option<Value> {
    let obj = v?.as_object()?;
    let mut out = Map::new();
    if let Some(b) = obj.get("chatCollapsed").and_then(Value::as_bool) {
        out.insert("chatCollapsed".into(), Value::Bool(b));
    }
    if out.is_empty() {
        None
    } else {
        Some(Value::Object(out))
    }
}

// ── Persistência ─────────────────────────────────────────────────────────

/// Paridade com `loadHumans()`. Retorna sempre um objeto válido mesmo
/// com JSON inválido no arquivo.
pub fn load_humans(mem_dir: &Path) -> Value {
    let path = humans_file(mem_dir);
    let Ok(raw) = fs::read_to_string(&path) else {
        return default_humans();
    };
    let Ok(v) = serde_json::from_str::<Value>(&raw) else {
        return default_humans();
    };
    if !v.is_object() {
        return default_humans();
    }
    let humans = v
        .get("humans")
        .filter(|x| x.is_object())
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    let default_id = v
        .get("default_id")
        .cloned()
        .filter(|x| x.is_string())
        .unwrap_or(Value::Null);
    let mut out = Map::new();
    out.insert("humans".into(), humans);
    out.insert("default_id".into(), default_id);
    Value::Object(out)
}

fn default_humans() -> Value {
    let mut out = Map::new();
    out.insert("humans".into(), Value::Object(Map::new()));
    out.insert("default_id".into(), Value::Null);
    Value::Object(out)
}

/// Grava `humans.json` com indent 2 (paridade `JSON.stringify(data, null, 2)`).
pub fn save_humans(mem_dir: &Path, data: &Value) -> Result<()> {
    fs::create_dir_all(mem_dir)?;
    let serialized = serde_json::to_string_pretty(data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(humans_file(mem_dir), serialized)?;
    Ok(())
}

/// Retorna `true` se `last_seen_at` está dentro da janela de online.
pub fn human_is_online(h: &Value, now_ms: i64) -> bool {
    let last = h
        .get("last_seen_at")
        .and_then(Value::as_str)
        .and_then(parse_iso_to_ms);
    match last {
        Some(ms) => (now_ms - ms) < ONLINE_WINDOW_MS,
        None => false,
    }
}

/// Summary do `humanSummary()` em `server.cjs:303`. `now_ms` decide `online`.
pub fn human_summary(h: &Value, now_ms: i64) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), h.get("id").cloned().unwrap_or(Value::Null));
    m.insert("name".into(), h.get("name").cloned().unwrap_or(Value::Null));
    m.insert(
        "description".into(),
        h.get("description")
            .cloned()
            .unwrap_or(Value::String(String::new())),
    );
    m.insert(
        "skills".into(),
        h.get("skills").cloned().unwrap_or(Value::Array(Vec::new())),
    );
    m.insert(
        "important".into(),
        h.get("important")
            .cloned()
            .unwrap_or(Value::String(String::new())),
    );
    let offline_privacy = h.get("offline_privacy").cloned().unwrap_or_else(|| {
        json!({ "never_share": [] })
    });
    m.insert("offline_privacy".into(), offline_privacy);
    m.insert(
        "location".into(),
        h.get("location").cloned().unwrap_or(Value::Null),
    );
    m.insert(
        "gender".into(),
        h.get("gender").cloned().unwrap_or(Value::Null),
    );
    m.insert("skin".into(), h.get("skin").cloned().unwrap_or(Value::Null));
    m.insert(
        "position".into(),
        h.get("position").cloned().unwrap_or(Value::Null),
    );
    m.insert(
        "ui_prefs".into(),
        h.get("ui_prefs").cloned().unwrap_or(Value::Null),
    );
    m.insert(
        "created_at".into(),
        h.get("created_at").cloned().unwrap_or(Value::Null),
    );
    m.insert(
        "last_seen_at".into(),
        h.get("last_seen_at").cloned().unwrap_or(Value::Null),
    );
    m.insert(
        "online".into(),
        Value::Bool(human_is_online(h, now_ms)),
    );
    m.insert(
        "notes".into(),
        h.get("notes")
            .cloned()
            .unwrap_or(Value::String(String::new())),
    );
    Value::Object(m)
}

/// Append-only log por humano. Silenciosa em erro (paridade `.cjs`).
pub fn append_human_log(mem_dir: &Path, id: &str, entry: &Value) {
    use std::io::Write;
    let dir = human_log_dir(mem_dir);
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = human_log_path(mem_dir, id);
    let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    let Ok(mut line) = serde_json::to_string(entry) else {
        return;
    };
    line.push('\n');
    let _ = f.write_all(line.as_bytes());
}

// ── Operações ────────────────────────────────────────────────────────────

/// Trunca em UTF-16 code units (paridade `String.prototype.slice` do JS).
pub fn slice_utf16(s: &str, n: usize) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    if units.len() <= n {
        return s.to_string();
    }
    String::from_utf16_lossy(&units[..n])
}

/// Normaliza um array de strings: cada item `.toString().slice(0, max_len)`,
/// depois cap do array em `max_items`.
fn sanitize_string_array(v: Option<&Value>, max_items: usize, max_len: usize) -> Vec<String> {
    let Some(arr) = v.and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .take(max_items)
        .map(|x| match x {
            Value::String(s) => slice_utf16(s, max_len),
            other => slice_utf16(&other.to_string(), max_len),
        })
        .collect()
}

/// Input pra criar um humano novo. `name` obrigatório — resto opcional.
#[derive(Debug, Clone, Default)]
pub struct CreateInput<'a> {
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub skills: Option<&'a Value>,
    pub important: Option<&'a str>,
    pub offline_privacy: Option<&'a Value>,
    pub notes: Option<&'a str>,
    pub location: Option<&'a Value>,
    pub gender: Option<&'a Value>,
    pub skin: Option<&'a Value>,
    pub position: Option<&'a Value>,
    pub ui_prefs: Option<&'a Value>,
}

/// Cria humano novo. Retorna o objeto bruto (não summary).
pub fn create_human(mem_dir: &Path, input: &CreateInput, clock: &dyn Clock) -> Result<Value> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(Error::NeedName);
    }
    let mut data = load_humans(mem_dir);
    let humans_map = data
        .get_mut("humans")
        .and_then(Value::as_object_mut)
        .expect("load_humans returns an object with 'humans'");

    let base = slug_human(input.name, clock.now_ms());
    let mut id = base.clone();
    let mut n: i64 = 2;
    while humans_map.contains_key(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    let now_iso = clock.now_iso();

    let mut h = Map::new();
    h.insert("id".into(), Value::String(id.clone()));
    h.insert("name".into(), Value::String(name.to_string()));
    h.insert(
        "description".into(),
        Value::String(slice_utf16(input.description.unwrap_or(""), 4000)),
    );
    h.insert(
        "skills".into(),
        Value::Array(
            sanitize_string_array(input.skills, 40, 60)
                .into_iter()
                .map(Value::String)
                .collect(),
        ),
    );
    h.insert(
        "important".into(),
        Value::String(slice_utf16(input.important.unwrap_or(""), 4000)),
    );
    let never_share: Vec<Value> = match input.offline_privacy.and_then(|v| v.as_object()) {
        Some(o) => sanitize_string_array(o.get("never_share"), 40, 200)
            .into_iter()
            .map(Value::String)
            .collect(),
        None => Vec::new(),
    };
    h.insert(
        "offline_privacy".into(),
        json!({ "never_share": never_share }),
    );
    h.insert(
        "notes".into(),
        Value::String(slice_utf16(input.notes.unwrap_or(""), 2000)),
    );
    h.insert(
        "location".into(),
        sanitize_location(input.location)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    h.insert(
        "gender".into(),
        sanitize_gender(input.gender)
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    h.insert(
        "skin".into(),
        sanitize_skin(input.skin).unwrap_or(Value::Null),
    );
    h.insert(
        "position".into(),
        sanitize_position(input.position).unwrap_or(Value::Null),
    );
    h.insert(
        "ui_prefs".into(),
        sanitize_ui_prefs(input.ui_prefs).unwrap_or(Value::Null),
    );
    h.insert("created_at".into(), Value::String(now_iso.clone()));
    h.insert("last_seen_at".into(), Value::String(now_iso.clone()));
    let human = Value::Object(h);

    humans_map.insert(id.clone(), human.clone());
    if data
        .get("default_id")
        .map(|v| v.is_null())
        .unwrap_or(true)
    {
        if let Some(obj) = data.as_object_mut() {
            obj.insert("default_id".into(), Value::String(id.clone()));
        }
    }
    save_humans(mem_dir, &data)?;
    append_human_log(
        mem_dir,
        &id,
        &json!({ "ts": now_iso, "type": "created", "name": name }),
    );
    Ok(human)
}

/// Marca `last_seen_at = now`. Retorna `Some(human)` ou `None` se não existe.
pub fn touch_human(mem_dir: &Path, id: &str, clock: &dyn Clock) -> Result<Option<Value>> {
    let mut data = load_humans(mem_dir);
    let Some(map) = data.get_mut("humans").and_then(Value::as_object_mut) else {
        return Ok(None);
    };
    if !map.contains_key(id) {
        return Ok(None);
    }
    let now_iso = clock.now_iso();
    if let Some(Value::Object(h)) = map.get_mut(id) {
        h.insert("last_seen_at".into(), Value::String(now_iso.clone()));
    }
    let out = map.get(id).cloned();
    save_humans(mem_dir, &data)?;
    Ok(out)
}

/// Update input — campos `None` não tocam; `Some(Value)` rewrites.
#[derive(Debug, Clone, Default)]
pub struct UpdateInput<'a> {
    pub name: Option<&'a Value>,
    pub description: Option<&'a Value>,
    pub skills: Option<&'a Value>,
    pub important: Option<&'a Value>,
    pub offline_privacy: Option<&'a Value>,
    pub notes: Option<&'a Value>,
    pub location: Option<&'a Value>,
    pub gender: Option<&'a Value>,
    pub skin: Option<&'a Value>,
    pub position: Option<&'a Value>,
    pub ui_prefs: Option<&'a Value>,
}

/// Atualiza humano em-place. Devolve o objeto atualizado ou [`Error::NotFound`].
pub fn update_human(
    mem_dir: &Path,
    id: &str,
    input: &UpdateInput,
    clock: &dyn Clock,
) -> Result<Value> {
    let mut data = load_humans(mem_dir);
    let map = data
        .get_mut("humans")
        .and_then(Value::as_object_mut)
        .ok_or(Error::NotFound)?;
    if !map.contains_key(id) {
        return Err(Error::NotFound);
    }

    let h = match map.get_mut(id) {
        Some(Value::Object(h)) => h,
        _ => return Err(Error::NotFound),
    };

    // Strings livres (name, description, important, notes) — slice 4000
    // (exceto notes=2000). `.cjs` trata todas iguais a 4000 no update
    // except notes (2000). Vamos respeitar isso.
    if let Some(v) = input.name {
        let s = value_to_string(v);
        h.insert("name".into(), Value::String(slice_utf16(&s, 4000)));
    }
    if let Some(v) = input.description {
        let s = value_to_string(v);
        h.insert("description".into(), Value::String(slice_utf16(&s, 4000)));
    }
    if let Some(v) = input.important {
        let s = value_to_string(v);
        h.insert("important".into(), Value::String(slice_utf16(&s, 4000)));
    }
    if let Some(v) = input.notes {
        let s = value_to_string(v);
        h.insert("notes".into(), Value::String(slice_utf16(&s, 4000)));
    }

    if let Some(v) = input.skills {
        if v.is_array() {
            let arr: Vec<Value> = sanitize_string_array(Some(v), 40, 60)
                .into_iter()
                .map(Value::String)
                .collect();
            h.insert("skills".into(), Value::Array(arr));
        }
        // Se não for array, `.cjs` mantém o valor antigo → noop.
    }
    if let Some(v) = input.offline_privacy {
        let ns = v
            .as_object()
            .and_then(|o| o.get("never_share"))
            .filter(|x| x.is_array());
        let existing = h
            .get("offline_privacy")
            .and_then(|op| op.get("never_share"))
            .cloned();
        let never_share: Vec<Value> = if ns.is_some() {
            sanitize_string_array(ns, 40, 200)
                .into_iter()
                .map(Value::String)
                .collect()
        } else {
            match existing {
                Some(Value::Array(a)) => a,
                _ => Vec::new(),
            }
        };
        h.insert(
            "offline_privacy".into(),
            json!({ "never_share": never_share }),
        );
    }
    if let Some(v) = input.location {
        h.insert(
            "location".into(),
            sanitize_location(Some(v))
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
    }
    if let Some(v) = input.gender {
        h.insert(
            "gender".into(),
            sanitize_gender(Some(v))
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
    }
    if let Some(v) = input.skin {
        h.insert("skin".into(), sanitize_skin(Some(v)).unwrap_or(Value::Null));
    }
    if let Some(v) = input.position {
        h.insert(
            "position".into(),
            sanitize_position(Some(v)).unwrap_or(Value::Null),
        );
    }
    if let Some(v) = input.ui_prefs {
        // `.cjs`: merge com `ui_prefs` anterior.
        let next = sanitize_ui_prefs(Some(v));
        let current = h.get("ui_prefs").and_then(Value::as_object).cloned();
        let merged = match (current, next) {
            (Some(mut cur), Some(Value::Object(n))) => {
                for (k, v) in n {
                    cur.insert(k, v);
                }
                Value::Object(cur)
            }
            (None, Some(v)) => v,
            (_, None) => Value::Null,
            _ => Value::Null,
        };
        h.insert("ui_prefs".into(), merged);
    }

    let now_iso = clock.now_iso();
    let updated = Value::Object(h.clone());
    save_humans(mem_dir, &data)?;
    append_human_log(
        mem_dir,
        id,
        &json!({ "ts": now_iso, "type": "updated" }),
    );
    Ok(updated)
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

pub fn delete_human(mem_dir: &Path, id: &str, clock: &dyn Clock) -> Result<String> {
    let mut data = load_humans(mem_dir);
    let map = data
        .get_mut("humans")
        .and_then(Value::as_object_mut)
        .ok_or(Error::NotFound)?;
    let removed = map.remove(id).ok_or(Error::NotFound)?;
    let name = removed
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    // Atualiza default_id se era esse.
    let default_was = data
        .get("default_id")
        .and_then(Value::as_str)
        .map(String::from);
    if default_was.as_deref() == Some(id) {
        let next_default = data
            .get("humans")
            .and_then(Value::as_object)
            .and_then(|m| m.keys().next().cloned());
        if let Some(obj) = data.as_object_mut() {
            obj.insert(
                "default_id".into(),
                next_default.map(Value::String).unwrap_or(Value::Null),
            );
        }
    }

    save_humans(mem_dir, &data)?;
    append_human_log(
        mem_dir,
        id,
        &json!({ "ts": clock.now_iso(), "type": "deleted", "name": name }),
    );
    Ok(id.to_string())
}

/// Lista todos os humans (já ordenado por name.localeCompare).
pub fn list_humans(mem_dir: &Path, now_ms: i64) -> Value {
    let data = load_humans(mem_dir);
    let empty_map = Map::new();
    let map = data
        .get("humans")
        .and_then(Value::as_object)
        .unwrap_or(&empty_map);
    let mut list: Vec<Value> = map.values().map(|h| human_summary(h, now_ms)).collect();
    list.sort_by(|a, b| {
        let na = a.get("name").and_then(Value::as_str).unwrap_or("");
        let nb = b.get("name").and_then(Value::as_str).unwrap_or("");
        na.cmp(nb)
    });
    let default_id = data.get("default_id").cloned().unwrap_or(Value::Null);
    json!({
        "humans": list,
        "default_id": default_id,
    })
}

/// Get + summary. `None` se não existe.
pub fn get_human_summary(mem_dir: &Path, id: &str, now_ms: i64) -> Option<Value> {
    let data = load_humans(mem_dir);
    let h = data.get("humans").and_then(|m| m.get(id))?;
    Some(human_summary(h, now_ms))
}

// ── Chat helpers ─────────────────────────────────────────────────────────

/// Paridade com `humanChatsDir = path.join(memDir, 'human_chats')`.
pub fn human_chats_dir(mem_dir: &Path) -> PathBuf {
    mem_dir.join("human_chats")
}

/// Paridade com `chatDir = path.join(memDir, 'agent_chats')`.
pub fn agent_chats_dir(mem_dir: &Path) -> PathBuf {
    mem_dir.join("agent_chats")
}

/// Paridade com `humanPairKey(a,b) = [a,b].sort().join('__')`.
pub fn human_pair_key(a: &str, b: &str) -> String {
    let mut pair = [a.to_string(), b.to_string()];
    pair.sort();
    format!("{}__{}", pair[0], pair[1])
}

/// Paridade com `humanChatPath`.
pub fn human_chat_path(mem_dir: &Path, a: &str, b: &str) -> PathBuf {
    human_chats_dir(mem_dir).join(format!("{}.jsonl", human_pair_key(a, b)))
}

/// Paridade com `readHumanChat`. Retorna vetor (vazio se arquivo não existe).
/// Linhas JSON inválidas são silenciosamente ignoradas.
pub fn read_human_chat(mem_dir: &Path, a: &str, b: &str) -> Vec<Value> {
    let path = human_chat_path(mem_dir, a, b);
    let Ok(raw) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    raw.split('\n')
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

/// Append-only em human_chats (usado por handoff — F4-05f). Silencia erros.
pub fn append_human_chat(mem_dir: &Path, a: &str, b: &str, entry: &Value) {
    use std::io::Write;
    let dir = human_chats_dir(mem_dir);
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = human_chat_path(mem_dir, a, b);
    let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    let Ok(mut line) = serde_json::to_string(entry) else {
        return;
    };
    line.push('\n');
    let _ = f.write_all(line.as_bytes());
}

/// Paridade com `handleHumanChatsSummary`. Scan de `agent_chats/*.jsonl` e
/// `human_chats/*.jsonl` filtrando pelo humano `hid`. Retorna
/// `{ human_id, agent_chats: [...], human_chats: [...] }`.
pub fn summarize_human_chats(mem_dir: &Path, hid: &str) -> Value {
    let agent_chats = scan_agent_chats(mem_dir, hid);
    let human_chats = scan_human_chats(mem_dir, hid);
    json!({
        "human_id": hid,
        "agent_chats": agent_chats,
        "human_chats": human_chats,
    })
}

fn scan_agent_chats(mem_dir: &Path, hid: &str) -> Vec<Value> {
    let dir = agent_chats_dir(mem_dir);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<Value> = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if !name.ends_with(".jsonl") {
            continue;
        }
        let agent_name = name.trim_end_matches(".jsonl").to_string();
        let Ok(raw) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let mut count: i64 = 0;
        let mut last_ts: Option<String> = None;
        let mut last_preview = String::new();
        let mut last_role: Option<String> = None;
        for line in raw.split('\n').filter(|l| !l.is_empty()) {
            let Ok(e) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let role = e.get("role").and_then(Value::as_str);
            if role == Some("user")
                && e.get("human_id").and_then(Value::as_str) != Some(hid)
            {
                continue;
            }
            if role == Some("agent") && count == 0 {
                continue;
            }
            count += 1;
            if let Some(ts) = e.get("ts").and_then(Value::as_str) {
                last_ts = Some(ts.to_string());
            }
            let content = e
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("");
            last_preview = slice_utf16(content, 140);
            last_role = role.map(str::to_string);
        }
        if count > 0 {
            out.push(json!({
                "agent": agent_name,
                "count": count,
                "last_ts": last_ts.map(Value::String).unwrap_or(Value::Null),
                "last_preview": last_preview,
                "last_role": last_role.map(Value::String).unwrap_or(Value::Null),
            }));
        }
    }
    // Ordena desc por last_ts (paridade `b.last_ts.localeCompare(a.last_ts)`).
    out.sort_by(|a, b| {
        let la = a.get("last_ts").and_then(Value::as_str).unwrap_or("");
        let lb = b.get("last_ts").and_then(Value::as_str).unwrap_or("");
        lb.cmp(la)
    });
    out
}

fn scan_human_chats(mem_dir: &Path, hid: &str) -> Vec<Value> {
    let dir = human_chats_dir(mem_dir);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    // `.cjs` carrega todos os profiles pra resolver peer_name.
    let data = load_humans(mem_dir);
    let empty_map = Map::new();
    let profiles = data
        .get("humans")
        .and_then(Value::as_object)
        .unwrap_or(&empty_map);
    let name_of = |id: &str| -> String {
        profiles
            .get(id)
            .and_then(|p| p.get("name"))
            .and_then(Value::as_str)
            .unwrap_or(id)
            .to_string()
    };

    let mut out: Vec<Value> = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if !name.ends_with(".jsonl") {
            continue;
        }
        let stem = name.trim_end_matches(".jsonl");
        let pair: Vec<&str> = stem.split("__").collect();
        if pair.len() != 2 || !pair.contains(&hid) {
            continue;
        }
        let peer_id = if pair[0] == hid { pair[1] } else { pair[0] };
        let Ok(raw) = fs::read_to_string(entry.path()) else {
            continue;
        };
        let mut count: i64 = 0;
        let mut last_ts: Option<String> = None;
        let mut last_preview = String::new();
        let mut last_from: Option<String> = None;
        for line in raw.split('\n').filter(|l| !l.is_empty()) {
            let Ok(e) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            count += 1;
            if let Some(ts) = e.get("ts").and_then(Value::as_str) {
                last_ts = Some(ts.to_string());
            }
            let content = e.get("content").and_then(Value::as_str).unwrap_or("");
            last_preview = slice_utf16(content, 140);
            last_from = e
                .get("from")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if count > 0 {
            out.push(json!({
                "peer_id": peer_id,
                "peer_name": name_of(peer_id),
                "count": count,
                "last_ts": last_ts.map(Value::String).unwrap_or(Value::Null),
                "last_preview": last_preview,
                "last_from_me": last_from.as_deref() == Some(hid),
            }));
        }
    }
    out.sort_by(|a, b| {
        let la = a.get("last_ts").and_then(Value::as_str).unwrap_or("");
        let lb = b.get("last_ts").and_then(Value::as_str).unwrap_or("");
        lb.cmp(la)
    });
    out
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fixed() -> FixedClock {
        // 2026-04-19T00:00:00.000Z em epoch ms.
        FixedClock(1_776_211_200_000)
    }

    #[test]
    fn slug_empty_fallback() {
        let s = slug_human("", 1_234_567);
        assert_eq!(s, "human-1234567");
    }

    #[test]
    fn slug_basic_kebab() {
        assert_eq!(slug_human("Tiago", 0), "tiago");
        assert_eq!(slug_human("Tiago Silva", 0), "tiago-silva");
    }

    #[test]
    fn slug_strips_diacritics() {
        assert_eq!(slug_human("José", 0), "jose");
        assert_eq!(slug_human("João Ávila", 0), "joao-avila");
    }

    #[test]
    fn slug_truncates_to_40_chars() {
        let long = "a".repeat(60);
        let s = slug_human(&long, 0);
        assert_eq!(s.len(), 40);
    }

    #[test]
    fn load_missing_returns_default() {
        let d = tempdir().unwrap();
        let v = load_humans(d.path());
        assert_eq!(v["humans"], json!({}));
        assert_eq!(v["default_id"], Value::Null);
    }

    #[test]
    fn load_malformed_json_returns_default() {
        let d = tempdir().unwrap();
        fs::write(humans_file(d.path()), "not json").unwrap();
        let v = load_humans(d.path());
        assert_eq!(v["humans"], json!({}));
    }

    #[test]
    fn create_and_list() {
        let d = tempdir().unwrap();
        let c = fixed();
        let h = create_human(
            d.path(),
            &CreateInput {
                name: "Tiago",
                ..Default::default()
            },
            &c,
        )
        .unwrap();
        assert_eq!(h["id"], "tiago");
        assert_eq!(h["name"], "Tiago");
        assert_eq!(h["last_seen_at"], h["created_at"]);

        let list = list_humans(d.path(), c.now_ms());
        assert_eq!(list["humans"].as_array().unwrap().len(), 1);
        assert_eq!(list["default_id"], "tiago");
    }

    #[test]
    fn create_rejects_empty_name() {
        let d = tempdir().unwrap();
        let err = create_human(
            d.path(),
            &CreateInput {
                name: "   ",
                ..Default::default()
            },
            &fixed(),
        )
        .unwrap_err();
        assert!(matches!(err, Error::NeedName));
    }

    #[test]
    fn create_collision_appends_suffix() {
        let d = tempdir().unwrap();
        let c = fixed();
        let a = create_human(
            d.path(),
            &CreateInput {
                name: "Tiago",
                ..Default::default()
            },
            &c,
        )
        .unwrap();
        let b = create_human(
            d.path(),
            &CreateInput {
                name: "Tiago",
                ..Default::default()
            },
            &c,
        )
        .unwrap();
        assert_eq!(a["id"], "tiago");
        assert_eq!(b["id"], "tiago-2");
    }

    #[test]
    fn touch_updates_last_seen() {
        let d = tempdir().unwrap();
        let c0 = FixedClock(1_000_000_000_000);
        create_human(
            d.path(),
            &CreateInput {
                name: "Tiago",
                ..Default::default()
            },
            &c0,
        )
        .unwrap();
        let c1 = FixedClock(1_000_000_060_000);
        let h = touch_human(d.path(), "tiago", &c1).unwrap().unwrap();
        assert_eq!(h["last_seen_at"], c1.now_iso());
    }

    #[test]
    fn update_respects_noop_for_undefined_fields() {
        let d = tempdir().unwrap();
        let c = fixed();
        create_human(
            d.path(),
            &CreateInput {
                name: "Tiago",
                description: Some("dev"),
                ..Default::default()
            },
            &c,
        )
        .unwrap();
        // UpdateInput sem campos → descrição preservada.
        let upd = update_human(d.path(), "tiago", &UpdateInput::default(), &c).unwrap();
        assert_eq!(upd["description"], "dev");
    }

    #[test]
    fn delete_clears_default_if_same_id() {
        let d = tempdir().unwrap();
        let c = fixed();
        create_human(
            d.path(),
            &CreateInput {
                name: "Tiago",
                ..Default::default()
            },
            &c,
        )
        .unwrap();
        delete_human(d.path(), "tiago", &c).unwrap();
        let v = load_humans(d.path());
        assert_eq!(v["default_id"], Value::Null);
    }

    #[test]
    fn sanitize_location_allowed_only() {
        assert_eq!(
            sanitize_location(Some(&json!("core"))),
            Some("core".into())
        );
        assert_eq!(sanitize_location(Some(&json!("moon"))), None);
        assert_eq!(sanitize_location(Some(&json!(null))), None);
    }

    #[test]
    fn sanitize_skin_defaults_applied_when_partial() {
        let v = sanitize_skin(Some(&json!({ "skinTone": 16_777_215 }))).unwrap();
        assert_eq!(v["skinTone"], 16_777_215);
        assert_eq!(v["hair"], 0x44_403c);
        assert_eq!(v["outfit"], "pants");
    }

    #[test]
    fn sanitize_skin_all_absent_returns_none() {
        assert!(sanitize_skin(Some(&json!({ "skinTone": -1 }))).is_none());
    }

    #[test]
    fn online_window_check() {
        let now = 1_000_000_000_000;
        let online_iso = format_iso_millis(now - 30_000);
        let offline_iso = format_iso_millis(now - 120_000);
        let h_on = json!({ "last_seen_at": online_iso });
        let h_off = json!({ "last_seen_at": offline_iso });
        assert!(human_is_online(&h_on, now));
        assert!(!human_is_online(&h_off, now));
    }

    #[test]
    fn human_pair_key_sorts() {
        assert_eq!(human_pair_key("bob", "alice"), "alice__bob");
        assert_eq!(human_pair_key("alice", "bob"), "alice__bob");
        assert_eq!(human_pair_key("zed", "zed"), "zed__zed");
    }

    #[test]
    fn read_human_chat_missing_file_returns_empty() {
        let d = tempdir().unwrap();
        let v = read_human_chat(d.path(), "alice", "bob");
        assert_eq!(v, Vec::<Value>::new());
    }

    #[test]
    fn append_then_read_human_chat_roundtrip() {
        let d = tempdir().unwrap();
        append_human_chat(
            d.path(),
            "alice",
            "bob",
            &json!({ "ts": "2026-04-19T00:00:00.000Z", "from": "alice", "content": "hi" }),
        );
        append_human_chat(
            d.path(),
            "bob",
            "alice",
            &json!({ "ts": "2026-04-19T00:00:01.000Z", "from": "bob", "content": "hey" }),
        );
        // Ambas chamadas gravam no mesmo arquivo (pair key é ordenada).
        let msgs = read_human_chat(d.path(), "alice", "bob");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["from"], "alice");
        assert_eq!(msgs[1]["from"], "bob");
        // Skip linha inválida no meio.
        let p = human_chat_path(d.path(), "alice", "bob");
        let mut raw = fs::read_to_string(&p).unwrap();
        raw.push_str("not-json\n");
        fs::write(&p, raw).unwrap();
        let msgs = read_human_chat(d.path(), "alice", "bob");
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn summarize_empty_dirs() {
        let d = tempdir().unwrap();
        let v = summarize_human_chats(d.path(), "tiago");
        assert_eq!(v["human_id"], "tiago");
        assert_eq!(v["agent_chats"], json!([]));
        assert_eq!(v["human_chats"], json!([]));
    }

    #[test]
    fn summarize_human_chats_filters_by_hid() {
        let d = tempdir().unwrap();
        // Cria perfis pra nome resolution.
        let c = fixed();
        for name in ["Alice", "Bob", "Eve"] {
            create_human(
                d.path(),
                &CreateInput { name, ..Default::default() },
                &c,
            )
            .unwrap();
        }
        // alice↔bob 2 mensagens.
        append_human_chat(
            d.path(),
            "alice",
            "bob",
            &json!({ "ts": "2026-04-19T00:00:00.000Z", "from": "alice", "content": "hi bob" }),
        );
        append_human_chat(
            d.path(),
            "alice",
            "bob",
            &json!({ "ts": "2026-04-19T00:00:05.000Z", "from": "bob", "content": "hi alice" }),
        );
        // bob↔eve 1 mensagem (não deve aparecer pra alice).
        append_human_chat(
            d.path(),
            "bob",
            "eve",
            &json!({ "ts": "2026-04-19T00:00:10.000Z", "from": "eve", "content": "yo" }),
        );
        let v = summarize_human_chats(d.path(), "alice");
        let hc = v["human_chats"].as_array().unwrap();
        assert_eq!(hc.len(), 1);
        assert_eq!(hc[0]["peer_id"], "bob");
        assert_eq!(hc[0]["peer_name"], "Bob");
        assert_eq!(hc[0]["count"], 2);
        assert_eq!(hc[0]["last_from_me"], false); // last was from bob
    }

    #[test]
    fn summarize_agent_chats_skips_leading_agent_messages() {
        let d = tempdir().unwrap();
        let dir = agent_chats_dir(d.path());
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("foo.jsonl");
        let mut raw = String::new();
        // agent msg antes de qualquer user → ignorado.
        raw.push_str(r#"{"role":"agent","ts":"a","content":"ignored"}"#);
        raw.push('\n');
        // user do outro humano → ignorado.
        raw.push_str(r#"{"role":"user","human_id":"other","ts":"b","content":"other"}"#);
        raw.push('\n');
        // user do hid → conta.
        raw.push_str(r#"{"role":"user","human_id":"me","ts":"c","content":"hi"}"#);
        raw.push('\n');
        // agent depois → conta.
        raw.push_str(r#"{"role":"agent","ts":"d","content":"reply"}"#);
        raw.push('\n');
        fs::write(&p, raw).unwrap();
        let v = summarize_human_chats(d.path(), "me");
        let ac = v["agent_chats"].as_array().unwrap();
        assert_eq!(ac.len(), 1);
        assert_eq!(ac[0]["agent"], "foo");
        assert_eq!(ac[0]["count"], 2);
        assert_eq!(ac[0]["last_ts"], "d");
        assert_eq!(ac[0]["last_role"], "agent");
        assert_eq!(ac[0]["last_preview"], "reply");
    }
}
