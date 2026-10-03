use mlua::{Lua, Thread, thread::ThreadStatus};

use crate::scripting::userdata::player::LuaPlayer;

#[derive(Debug, Default)]
pub struct Scheduler {
    pending: Vec<(Thread, u64)>,
    tick: u64,
}

impl Scheduler {
    #[allow(dead_code)]
    pub fn start(
        &mut self,
        lua: &Lua,
        func: mlua::Function,
        player: LuaPlayer,
    ) -> mlua::Result<()> {
        let thread = lua.create_thread(func)?;

        self.resume(thread, player)
    }

    pub fn start_thread(
        &mut self,
        thread: Thread,
        args: impl mlua::IntoLuaMulti,
    ) -> mlua::Result<()> {
        self.resume(thread, args)
    }

    fn resume(&mut self, thread: Thread, args: impl mlua::IntoLuaMulti) -> mlua::Result<()> {
        let result: Option<f64> = thread.resume(args)?;

        if thread.status() == ThreadStatus::Resumable {
            let ticks = (result.unwrap_or(0.0) * 1.0) as u64;
            self.pending.push((thread, self.tick + ticks));
        }

        Ok(())
    }

    pub fn on_tick(&mut self) {
        self.tick += 1;

        let ready: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, at)| *at <= self.tick)
            .cloned()
            .collect();
        self.pending.retain(|(_, at)| *at > self.tick);

        for (thread, _) in ready {
            let _ = thread.resume::<Option<f64>>(());
        }
    }
}
