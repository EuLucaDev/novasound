//! Colaboração em tempo real (Nova Live) no motor do Nova Sound.
//!
//! - Comandos do diário (faixas, regiões, mixagem, automação) viram [`LiveChange::Op`]; a seleção
//!   e o intervalo de edição fazem parte da sessão de áudio, então são compartilhados.
//! - Qualquer outra mudança (desfazer, refazer, gravação, gestos) vira [`LiveChange::Replace`].
//! - Os arquivos de áudio ficam no computador de cada um: a sessão sincroniza a montagem; áudio
//!   que falta aparece offline.

use std::sync::Arc;

use novasound_model::Session as Doc;
use novasound_model::SourceAudio;
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
    /// Áudios recebidos de colegas, pelo caminho que a sessão usa (o áudio decodificado não faz
    /// parte do documento: volta ao pool depois de carregar um documento novo ou de desfazer).
    pub(crate) received: std::collections::HashMap<String, Arc<SourceAudio>>,
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
        let received = std::mem::take(&mut self.live.received);
        self.live = LiveState { on: true, seen: Some(self.doc.clone()), received, ..LiveState::default() };
        Ok(())
    }

    pub fn live_stop(&mut self) {
        let received = std::mem::take(&mut self.live.received);
        self.live = LiveState { received, ..LiveState::default() };
    }

    /// Arquivos de áudio que a sessão usa, pelo caminho guardado nela.
    pub fn live_media(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for src in &self.doc.sources {
            if !src.path.is_empty() && !out.contains(&src.path) {
                out.push(src.path.clone());
            }
        }
        out
    }

    /// O áudio desta chave está carregado aqui?
    pub fn live_media_available(&mut self, path: &str) -> bool {
        self.restore_received();
        let mut any = false;
        for src in self.doc.sources.iter().filter(|s| s.path == path) {
            any = true;
            if !self.doc.pool.contains(src.id) {
                return false;
            }
        }
        any
    }

    /// O áudio desta chave em WAV (32 bits flutuante), para enviar a um colega.
    pub fn live_media_wav(&self, path: &str) -> Option<Vec<u8>> {
        let audio = self.doc.sources.iter().filter(|s| s.path == path).find_map(|s| self.doc.pool.get(s.id))?;
        let opts = novasound_audio_io::EncodeOptions { bit_depth: novasound_audio_io::BitDepth::Float32, ..Default::default() };
        novasound_audio_io::encode(&audio.buffer, &opts).ok()
    }

    /// O áudio chegou (WAV em `local`): entra no pool das fontes com esse caminho.
    pub fn live_media_ready(&mut self, path: &str, local: &std::path::Path) -> Result<()> {
        let bytes = std::fs::read(local).map_err(err)?;
        let audio = crate::io::decode_source(&bytes, Some("wav"), self.doc.sample_rate.hz()).map_err(err)?;
        self.live.received.insert(path.to_string(), audio);
        self.restore_received();
        Ok(())
    }

    /// Põe de volta no pool os áudios recebidos que o documento atual usa e não tem carregados.
    fn restore_received(&mut self) {
        if self.live.received.is_empty() {
            return;
        }
        let missing: Vec<_> = self
            .doc
            .sources
            .iter()
            .filter(|s| !self.doc.pool.contains(s.id))
            .filter_map(|s| self.live.received.get(&s.path).map(|a| (s.id, a.clone())))
            .collect();
        if missing.is_empty() {
            return;
        }
        let tracked = self.live.seen.as_ref().is_some_and(|s| Arc::ptr_eq(s, &self.doc));
        let d = Arc::make_mut(&mut self.doc);
        for (id, audio) in missing {
            d.pool.insert(id, audio);
        }
        // Só o áudio (fora do documento) mudou: não é uma alteração para enviar.
        if tracked {
            self.live.seen = Some(self.doc.clone());
        }
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
        let mut d: Doc = serde_json::from_slice(bytes).map_err(err)?;
        // O áudio decodificado não viaja no documento: fica o que já estava carregado aqui.
        d.pool = self.doc.pool.clone();
        d.folder = self.doc.folder.clone();
        self.doc = Arc::new(d);
        self.revision = self.revision.wrapping_add(1);
        self.live.seen = Some(self.doc.clone());
        self.restore_received();
        // O desfazer guarda fotos da sessão: depois de uma alteração de colega, desfazer o passo
        // anterior apagaria o trabalho dele. O histórico local recomeça daqui.
        self.undo.clear();
        self.redo.clear();
        Ok(())
    }

    /// Roda o comando de um colega; o desfazer local não ganha o passo dele.
    pub fn live_apply(&mut self, cmd: &str, params: &Value, _ctx: &Value) -> Result<()> {
        if !remote_command_allowed(cmd) {
            return Err(err("comando não permitido na colaboração"));
        }
        self.live.applying = true;
        let r = self.execute(cmd, params);
        self.live.applying = false;
        // Desfazer não pode apagar o passo do colega: o histórico local recomeça daqui.
        self.undo.clear();
        self.redo.clear();
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
