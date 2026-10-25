use mlua::{Thread, thread::ThreadStatus};
use paris::error;

use crate::scripting::api::TICK_RATE;

#[derive(Debug, Default)]
pub struct Scheduler {
    pending: Vec<(Thread, u64)>,
    tick: u64,
}

impl Scheduler {
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
            let ticks = (result.unwrap_or(0.0) * TICK_RATE).round().max(1.0) as u64;
            self.pending.push((thread, self.tick + ticks));
        }

        Ok(())
    }

    pub fn on_tick(&mut self) {
        self.tick += 1;

        let (ready, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|(_, at)| *at <= self.tick);

        self.pending = waiting;

        for (thread, _) in ready {
            if let Err(err) = self.resume(thread, ()) {
                error!("lua task error: {err}");
            }
        }
    }
}
