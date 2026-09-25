use mlua::{FromLua, Function, Lua, Table, Value};

use crate::fs::tank_defs::{Barrel, Bullet, Stat, Tank, TankFlags};

pub struct WeaponDef {
    pub damage: f32,
    pub reload: f32,
    pub speed: f32,
    pub on_shoot: Function,
}

pub fn load_weapon(lua: &Lua, path: &str) -> mlua::Result<WeaponDef> {
    let table: mlua::Table = lua.load(&std::fs::read_to_string(path)?).eval()?;
    Ok(WeaponDef {
        damage: table.get("damage")?,
        reload: table.get("reload")?,
        speed: table.get("speed")?,
        on_shoot: table.get("onShoot")?,
    })
}

pub struct TankDef {
    pub tank: Tank,
    pub upgrade_names: Vec<String>,
    pub on_shoot: Option<Function>,
}

fn get_or<T: FromLua>(lua: &Lua, table: &Table, key: &str, default: T) -> mlua::Result<T> {
    match table.get(key)? {
        Value::Nil => Ok(default),
        value => T::from_lua(value, lua),
    }
}

impl FromLua for TankFlags {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        let t = Table::from_lua(value, lua)?;
        Ok(Self {
            invisibility: get_or(lua, &t, "invisibility", false)?,
            zoom_ability: get_or(lua, &t, "zoomAbility", false)?,
            can_shoot: get_or(lua, &t, "canShoot", true)?,
            dev_only: get_or(lua, &t, "devOnly", false)?,
        })
    }
}

impl FromLua for Bullet {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        let t = Table::from_lua(value, lua)?;
        Ok(Self {
            bullet_type: get_or(lua, &t, "type", "bullet".to_string())?,
            health: get_or(lua, &t, "health", 1.0)?,
            damage: get_or(lua, &t, "damage", 1.0)?,
            speed: get_or(lua, &t, "speed", 1.0)?,
            scatter_rate: get_or(lua, &t, "scatterRate", 1.0)?,
            life_length: get_or(lua, &t, "lifeLength", 1.0)?,
            absorbtion_factor: get_or(lua, &t, "absorbtionFactor", 1.0)?,
            size_ratio: get_or(lua, &t, "sizeRatio", 1.0)?,
        })
    }
}

impl FromLua for Stat {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        let t = Table::from_lua(value, lua)?;
        Ok(Self {
            name: get_or(lua, &t, "name", String::new())?,
            max: get_or(lua, &t, "max", 7)?,
        })
    }
}

impl FromLua for Barrel {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        let t = Table::from_lua(value, lua)?;
        Ok(Self {
            x: get_or(lua, &t, "x", 0.0)?,
            y: get_or(lua, &t, "y", 0.0)?,
            angle: get_or(lua, &t, "angle", 0.0)?,
            width: get_or(lua, &t, "width", 18.0)?,
            length: get_or(lua, &t, "length", 40.0)?,
            delay: get_or(lua, &t, "delay", 0.0)?,
            reload: get_or(lua, &t, "reload", 1.0)?,
            recoil: get_or(lua, &t, "recoil", 1.0)?,
            is_trapezoid: get_or(lua, &t, "isTrapezoid", false)?,
            trapezoid_direction: get_or(lua, &t, "trapezoidDirection", 0.0)?,
            addon: get_or(lua, &t, "addon", 0.0)?,
            bullet: t.get("bullet")?,
            drone_count: t.get("droneCount")?,
        })
    }
}

impl FromLua for TankDef {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        let t = Table::from_lua(value, lua)?;
        Ok(Self {
            tank: Tank {
                id: get_or(lua, &t, "id", 0)?,
                name: t.get("name")?,
                upgrade_message: get_or(lua, &t, "upgradeMessage", String::new())?,
                level_requirement: get_or(lua, &t, "levelRequirement", 0)?,
                upgrades: Vec::new(),
                flags: get_or(
                    lua,
                    &t,
                    "flags",
                    TankFlags {
                        invisibility: false,
                        zoom_ability: false,
                        can_shoot: true,
                        dev_only: false,
                    },
                )?,
                field_factor: get_or(lua, &t, "fieldFactor", 1.0)?,
                absorbtion_factor: get_or(lua, &t, "absorbtionFactor", 1.0)?,
                max_health: get_or(lua, &t, "maxHealth", 50)?,
                pre_addon: get_or(lua, &t, "preAddon", 0)?,
                post_addon: get_or(lua, &t, "postAddon", 0)?,
                sides: get_or(lua, &t, "sides", 1)?,
                speed: get_or(lua, &t, "speed", 1.0)?,
                barrels: get_or(lua, &t, "barrels", Vec::new())?,
                stats: get_or(lua, &t, "stats", Vec::new())?,
            },
            upgrade_names: get_or(lua, &t, "upgrades", Vec::new())?,
            on_shoot: t.get("onShoot")?,
        })
    }
}

pub fn load_tank(lua: &Lua, path: &str) -> mlua::Result<TankDef> {
    let src = std::fs::read_to_string(path)?;
    lua.load(&src).set_name(path).eval::<TankDef>()
}
