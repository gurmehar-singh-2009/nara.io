use mlua::{FromLua, Function, Lua, Table, Value};
use shared::packets::client_bound::TankSpec;

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
    pub spec: TankSpec,
    pub on_shoot: Function,
}

impl FromLua for TankDef {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        let table = Table::from_lua(value, lua)?;
        Ok(TankDef {
            spec: TankSpec {
                name: table.get("name")?,
                health: table.get("health")?,
                speed: table.get("speed")?,
                barrels: table.get("barrels")?,
            },
            on_shoot: table.get("onShoot")?,
        })
    }
}

pub fn load_tank(lua: &Lua, path: &str) -> mlua::Result<TankDef> {
    let src = std::fs::read_to_string(path)?;
    lua.load(&src).set_name(path).eval::<TankDef>()
}
