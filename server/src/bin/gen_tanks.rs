use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

fn d_zero() -> f64 {
    0.0
}
fn d_one() -> f64 {
    1.0
}
fn d_size() -> f64 {
    95.0
}
fn d_max_health() -> u32 {
    50
}
fn d_sides() -> u32 {
    1
}
fn d_bullet_type() -> String {
    "bullet".into()
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct JsonTank {
    id: u32,
    name: String,
    #[serde(default)]
    upgrade_message: String,
    #[serde(default)]
    level_requirement: u32,
    #[serde(default)]
    upgrades: Vec<u32>,
    #[serde(default)]
    flags: JsonTankFlags,
    #[serde(default = "d_one")]
    field_factor: f64,
    #[serde(default = "d_one")]
    absorbtion_factor: f64,
    #[serde(default = "d_max_health")]
    max_health: u32,
    #[serde(default)]
    pre_addon: u32,
    #[serde(default)]
    post_addon: u32,
    #[serde(default = "d_sides")]
    sides: u32,
    #[serde(default)]
    barrels: Vec<JsonBarrel>,
    #[serde(default)]
    stats: Vec<JsonStat>,
}

impl Default for JsonTankFlags {
    fn default() -> Self {
        Self {
            invisibility: false,
            zoom_ability: false,
            can_shoot: true,
            dev_only: false,
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct JsonTankFlags {
    invisibility: bool,
    zoom_ability: bool,
    can_shoot: bool,
    dev_only: bool,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct JsonBarrel {
    #[serde(default = "d_zero")]
    angle: f64,
    #[serde(default = "d_zero")]
    offset: f64,
    #[serde(default = "d_size")]
    size: f64,
    #[serde(default = "d_one")]
    width: f64,
    #[serde(default = "d_zero")]
    delay: f64,
    #[serde(default = "d_one")]
    reload: f64,
    #[serde(default = "d_one")]
    recoil: f64,
    #[serde(default)]
    flags: JsonBarrelFlags,
    #[serde(default = "d_zero")]
    trapezoid_direction: f64,
    #[serde(default = "d_zero")]
    addon: f64,
    #[serde(default)]
    bullet: JsonBullet,
    drone_count: Option<u32>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct JsonBarrelFlags {
    is_trapezoid: bool,
    force_fire: bool,
}

impl Default for JsonBullet {
    fn default() -> Self {
        Self {
            bullet_type: "bullet".into(),
            health: 1.0,
            damage: 1.0,
            speed: 1.0,
            scatter_rate: 1.0,
            life_length: 1.0,
            absorbtion_factor: 1.0,
            size_ratio: 1.0,
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
struct JsonBullet {
    #[serde(rename = "type", default = "d_bullet_type")]
    bullet_type: String,
    #[serde(default = "d_one")]
    health: f64,
    #[serde(default = "d_one")]
    damage: f64,
    #[serde(default = "d_one")]
    speed: f64,
    #[serde(default = "d_one")]
    scatter_rate: f64,
    #[serde(default = "d_one")]
    life_length: f64,
    #[serde(default = "d_one")]
    absorbtion_factor: f64,
    #[serde(default = "d_one")]
    size_ratio: f64,
}

#[derive(Deserialize, Debug, Clone)]
struct JsonStat {
    #[serde(default)]
    name: String,
    #[serde(default = "d_max_health")]
    max: u32,
}

const DEF_SCALE: f64 = 42.0 / 100.0;
const BASE_BARREL_WIDTH: f64 = 42.0;

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

fn num(v: f64) -> String {
    let v = (v * 10_000.0).round() / 10_000.0;
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v}")
    }
}

fn boolean(v: bool) -> &'static str {
    if v { "true" } else { "false" }
}

fn render_barrel(b: &JsonBarrel, out: &mut String) {
    let angle = b.angle.to_degrees();
    let y = b.offset * DEF_SCALE;
    let length = b.size * DEF_SCALE;
    let width = b.width * BASE_BARREL_WIDTH * DEF_SCALE;

    out.push_str("        {\n");
    out.push_str("            x = 0,\n");
    out.push_str(&format!("            y = {},\n", num(y)));
    out.push_str(&format!("            angle = {},\n", num(angle)));
    out.push_str(&format!("            width = {},\n", num(width)));
    out.push_str(&format!("            length = {},\n", num(length)));
    out.push_str(&format!("            delay = {},\n", num(b.delay)));
    out.push_str(&format!("            reload = {},\n", num(b.reload)));
    out.push_str(&format!("            recoil = {},\n", num(b.recoil)));
    out.push_str(&format!(
        "            isTrapezoid = {},\n",
        boolean(b.flags.is_trapezoid)
    ));
    out.push_str(&format!(
        "            trapezoidDirection = {},\n",
        num(b.trapezoid_direction)
    ));
    out.push_str(&format!("            addon = {},\n", num(b.addon)));
    if let Some(count) = b.drone_count {
        out.push_str(&format!("            droneCount = {},\n", count));
    }
    out.push_str("            bullet = {\n");
    out.push_str(&format!(
        "                type = \"{}\",\n",
        b.bullet.bullet_type
    ));
    out.push_str(&format!(
        "                health = {},\n",
        num(b.bullet.health)
    ));
    out.push_str(&format!(
        "                damage = {},\n",
        num(b.bullet.damage)
    ));
    out.push_str(&format!(
        "                speed = {},\n",
        num(b.bullet.speed)
    ));
    out.push_str(&format!(
        "                scatterRate = {},\n",
        num(b.bullet.scatter_rate)
    ));
    out.push_str(&format!(
        "                lifeLength = {},\n",
        num(b.bullet.life_length)
    ));
    out.push_str(&format!(
        "                absorbtionFactor = {},\n",
        num(b.bullet.absorbtion_factor)
    ));
    out.push_str(&format!(
        "                sizeRatio = {},\n",
        num(b.bullet.size_ratio)
    ));
    out.push_str("            },\n");
    out.push_str("        },\n");
}

fn render_tank(tank: &JsonTank, upgrades: &[String]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "-- {} (converted from tank_defs.json)\n",
        tank.name
    ));
    out.push_str("-- units are game units (tank body = 42), angles are degrees.\n");
    // out.push_str("-- the server hot-reloads this file within ~1 second of an edit.\n\n");
    out.push_str("return {\n");
    out.push_str(&format!("    id = {},\n", tank.id));
    out.push_str(&format!("    name = \"{}\",\n", tank.name));
    out.push_str(&format!(
        "    upgradeMessage = \"{}\",\n",
        tank.upgrade_message
    ));
    out.push_str(&format!(
        "    levelRequirement = {},\n",
        tank.level_requirement
    ));
    out.push_str("    upgrades = {");
    if upgrades.is_empty() {
        out.push_str("},\n");
    } else {
        out.push('\n');
        for up in upgrades {
            out.push_str(&format!("        \"{}\",\n", up));
        }
        out.push_str("    },\n");
    }
    out.push_str("    speed = 1,\n");
    out.push_str(&format!("    maxHealth = {},\n", tank.max_health));
    out.push_str(&format!("    sides = {},\n", tank.sides));
    out.push_str(&format!("    fieldFactor = {},\n", num(tank.field_factor)));
    out.push_str(&format!(
        "    absorbtionFactor = {},\n",
        num(tank.absorbtion_factor)
    ));
    out.push_str(&format!("    preAddon = {},\n", tank.pre_addon));
    out.push_str(&format!("    postAddon = {},\n", tank.post_addon));
    out.push_str("    flags = {\n");
    out.push_str(&format!(
        "        invisibility = {},\n",
        boolean(tank.flags.invisibility)
    ));
    out.push_str(&format!(
        "        zoomAbility = {},\n",
        boolean(tank.flags.zoom_ability)
    ));
    out.push_str(&format!(
        "        canShoot = {},\n",
        boolean(tank.flags.can_shoot)
    ));
    out.push_str(&format!(
        "        devOnly = {},\n",
        boolean(tank.flags.dev_only)
    ));
    out.push_str("    },\n");
    out.push_str("    stats = {\n");
    for stat in &tank.stats {
        out.push_str(&format!(
            "        {{ name = \"{}\", max = {} }},\n",
            stat.name, stat.max
        ));
    }
    out.push_str("    },\n");
    out.push_str("    barrels = {\n");
    for barrel in &tank.barrels {
        render_barrel(barrel, &mut out);
    }
    out.push_str("    },\n");
    out.push_str("    onShoot = function(player)\n");
    out.push_str("        -- TODO\n");
    out.push_str("    end,\n");
    out.push_str("}\n");
    out
}

fn main() {
    let json_path = [
        "src/fs/data/tank_defs.json",
        "server/src/fs/data/tank_defs.json",
        "../server/src/fs/data/tank_defs.json",
    ]
    .into_iter()
    .find(|p| Path::new(p).exists())
    .expect("tank_defs.json not found (run from the server crate root)");

    let tanks: Vec<JsonTank> = serde_json::from_str(
        &fs::read_to_string(json_path).expect("failed to read tank_defs.json"),
    )
    .expect("failed to parse tank_defs.json");

    let out_root = ["content", "../content", "server/content"]
        .into_iter()
        .find(|p| Path::new(p).exists())
        .unwrap_or("content");
    let out_dir = PathBuf::from(out_root).join("tanks");
    fs::create_dir_all(&out_dir).expect("failed to create content/tanks");

    let slug_by_id: HashMap<u32, String> = tanks.iter().map(|t| (t.id, slug(&t.name))).collect();

    let mut written = 0;
    for tank in &tanks {
        let upgrades: Vec<String> = tank
            .upgrades
            .iter()
            .filter_map(|id| slug_by_id.get(id).cloned())
            .collect();

        let path = out_dir.join(format!("{}.lua", slug(&tank.name)));
        fs::write(&path, render_tank(tank, &upgrades)).expect("failed to write tank file");
        written += 1;
    }

    println!(
        "wrote {} tank definitions to {}",
        written,
        out_dir.display()
    );
}
