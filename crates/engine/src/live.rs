//! Colaboração em tempo real (Nova Live) no motor do Nova Sound.
//!
//! - Comandos do diário (faixas, regiões, mixagem, automação) viram [`LiveChange::Op`]; a seleção
//!   e o intervalo de edição fazem parte da sessão de áudio, então são compartilhados.
//! - Qualquer outra mudança (desfazer, refazer, gravação, gestos) vira [`LiveChange::Replace`].
//! - Os arquivos de áudio ficam no computador de cada um: a sessão sincroniza a montagem; áudio
//!   que falta aparece offline.

use std::sync::Arc;

use novasound_model::Session as Doc;
use serde_json::Value;

use crate::{Engine, EngineError, Result};

#[derive(Clone, Debug, PartialEq)]
pub enum LiveChange {
    Op { cmd: String, params: Value, ctx: Value },
    Replace,
}

#[derive(Default)]
pub struct LiveState {
    pub on: bool,
    pub outbox: Vec<LiveChange>,
    pub(crate) applying: bool,
    pub(crate) seen: Option<Arc<Doc>>,
    pub(crate) depth: u32,
}

fn err(e: impl std::fmt::Display) -> EngineError {
    EngineError::Io(format!("colaboração: {e}"))
}

/// Impressão digital estável (FNV-1a 64).
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// Only audited document edits may execute on a peer. Other local edits travel as data.
fn remote_command_allowed(cmd: &str) -> bool {
    matches!(cmd, "track.new")
}

impl Engine {
    pub fn live_start(&mut self) -> Result<()> {
        self.live = LiveState { on: true, seen: Some(self.doc.clone()), ..LiveState::default() };
        Ok(())
    }

    pub fn live_stop(&mut self) {
        self.live = LiveState::default();
    }

    pub fn live_active(&self) -> bool {
        self.live.on
    }

    pub fn live_take(&mut self) -> Vec<LiveChange> {
        if self.live.on && !self.live.seen.as_ref().is_some_and(|s| Arc::ptr_eq(s, &self.doc)) {
            self.live.seen = Some(self.doc.clone());
            self.live.outbox.push(LiveChange::Replace);
        }
        std::mem::take(&mut self.live.outbox)
    }

    pub fn live_busy(&self) -> bool {
        false
    }

    pub fn live_snapshot(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&*self.doc).map_err(err)
    }

    pub fn live_digest(&self) -> Result<u64> {
        Ok(fnv64(&self.live_snapshot()?))
    }

    pub fn live_load(&mut self, bytes: &[u8]) -> Result<()> {
        let d: Doc = serde_json::from_slice(bytes).map_err(err)?;
        self.doc = Arc::new(d);
        self.revision = self.revision.wrapping_add(1);
        self.live.seen = Some(self.doc.clone());
        Ok(())
    }

    /// Roda o comando de um colega; o desfazer local não ganha o passo dele.
    pub fn live_apply(&mut self, cmd: &str, params: &Value, _ctx: &Value) -> Result<()> {
        if !remote_command_allowed(cmd) {
            return Err(err("comando não permitido na colaboração"));
        }
        let (undo, redo) = (self.undo.len(), self.redo.clone());
        self.live.applying = true;
        let r = self.execute(cmd, params);
        self.live.applying = false;
        self.undo.truncate(undo);
        self.redo = redo;
        self.live.seen = Some(self.doc.clone());
        r.map(|_| ())
    }

    pub(crate) fn live_record(&mut self, id: &str, params: &Value, journal: bool) {
        if self.live.applying || !self.live.on || self.live.seen.as_ref().is_some_and(|s| Arc::ptr_eq(s, &self.doc)) {
            return;
        }
        if journal && remote_command_allowed(id) && !matches!(id, "edit.undo" | "edit.redo") {
            self.live.outbox.push(LiveChange::Op { cmd: id.to_string(), params: params.clone(), ctx: Value::Null });
            self.live.seen = Some(self.doc.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use serde_json::json;

    #[test]
    fn remote_commands_cannot_open_export_or_execute_code() {
        let (_, mut b) = pair();
        let before = b.live_digest().unwrap();
        for command in ["file.open", "file.saveAs", "file.export", "session.open", "script.run", "app.quit", "lisp.eval"] {
            assert!(b.live_apply(command, &serde_json::json!({"path": "/tmp/not-authorized"}), &serde_json::json!({})).is_err());
            assert_eq!(before, b.live_digest().unwrap());
        }
    }

    fn pair() -> (Engine, Engine) {
        let mut a = Engine::default();
        a.live_start().unwrap();
        let mut b = Engine::default();
        b.live_start().unwrap();
        let snap = a.live_snapshot().unwrap();
        b.live_load(&snap).unwrap();
        (a, b)
    }

    fn sync(a: &mut Engine, b: &mut Engine) {
        for change in a.live_take() {
            match change {
                LiveChange::Op { cmd, params, ctx } => b.live_apply(&cmd, &params, &ctx).unwrap(),
                LiveChange::Replace => {
                    let snap = a.live_snapshot().unwrap();
                    b.live_load(&snap).unwrap();
                }
            }
        }
        assert!(b.live_take().is_empty(), "o que veio de A não volta para A");
    }

    #[test]
    fn tracks_replicate_and_undo_travels_as_a_session() {
        let (mut a, mut b) = pair();
        a.execute("track.new", &json!({})).unwrap();
        a.execute("track.new", &json!({})).unwrap();
        sync(&mut a, &mut b);
        assert_eq!(a.live_digest().unwrap(), b.live_digest().unwrap());

        a.execute("edit.undo", &json!({})).unwrap();
        sync(&mut a, &mut b);
        assert_eq!(a.live_digest().unwrap(), b.live_digest().unwrap());

        b.execute("track.new", &json!({})).unwrap();
        sync(&mut b, &mut a);
        assert_eq!(a.live_digest().unwrap(), b.live_digest().unwrap());
    }
}
