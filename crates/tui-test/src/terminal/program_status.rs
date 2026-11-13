#[cfg(feature = "ghostty")]
use anyhow::{bail, Result};
#[cfg(feature = "ghostty")]
use ghostty_vt::terminal::{
    ProgramStatus as Report, ProgramStatusKind as Kind, ProgramStatusState as State,
};

#[cfg(any(feature = "ghostty", test))]
use crate::api::ProgramStatusKind;
use crate::api::{ProgramState, ProgramStatus};

/// Native backends validate and decode reports. Retain owned records in update order.
#[derive(Default)]
pub(super) struct ProgramStatuses {
    records: Vec<ProgramStatus>,
}

impl ProgramStatuses {
    #[cfg(feature = "ghostty")]
    pub fn report(&mut self, report: Report<'_>) -> Result<()> {
        let id = std::str::from_utf8(report.id())?.to_owned();
        let state = match report.state()? {
            State::Clear => {
                self.clear(&id);
                return Ok(());
            }
            State::Idle => ProgramState::Idle,
            State::Working => ProgramState::Working,
            State::Done => ProgramState::Done,
            State::Blocked => ProgramState::Blocked,
            State::Error => ProgramState::Error,
            state => bail!("unsupported Ghostty program state: {state:?}"),
        };
        let kind = match report.kind()? {
            Kind::None => None,
            Kind::Permission => Some(ProgramStatusKind::Permission),
            Kind::Question => Some(ProgramStatusKind::Question),
            Kind::Auth => Some(ProgramStatusKind::Auth),
            kind => bail!("unsupported Ghostty program-status kind: {kind:?}"),
        };
        let record = ProgramStatus {
            id,
            state,
            kind,
            app: text(report.app())?,
            title: text(report.title())?,
            message: text(report.message())?,
            progress: report.progress(),
        };
        self.update(record);
        Ok(())
    }

    pub fn clear(&mut self, id: &str) {
        self.records.retain(|record| {
            !id.is_empty()
                && record.id != id
                && !record
                    .id
                    .strip_prefix(id)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        });
    }

    pub fn update(&mut self, record: ProgramStatus) {
        self.records.retain(|old| old.id != record.id);
        if self.records.len() == 256 {
            self.records.remove(0);
        }
        self.records.push(record);
    }

    pub fn finish_process(&mut self) {
        self.records.retain(|record| {
            !matches!(record.state, ProgramState::Working | ProgramState::Blocked)
        });
    }

    pub fn all(&self) -> Vec<ProgramStatus> {
        let mut records = self.records.clone();
        for record in &mut records {
            let mut parent = record.id.as_str();
            while record.app.is_none() && !parent.is_empty() {
                parent = parent.rsplit_once('/').map_or("", |(parent, _)| parent);
                record.app = self
                    .records
                    .iter()
                    .find(|record| record.id == parent)
                    .and_then(|record| record.app.clone());
            }
        }
        records.sort_by(|left, right| left.id.cmp(&right.id));
        records
    }
}

#[cfg(feature = "ghostty")]
fn text(bytes: &[u8]) -> Result<Option<String>> {
    Ok((!bytes.is_empty())
        .then(|| std::str::from_utf8(bytes).map(str::to_owned))
        .transpose()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{profile::Profile, terminal::emu::Emulator, Backend};

    fn emulators() -> impl Iterator<Item = (Backend, Box<dyn Emulator>)> {
        Backend::ALL
            .iter()
            .copied()
            .filter(|backend| matches!(backend.as_str(), "ghostty" | "rio"))
            .map(|backend| (backend, backend.build(80, 24, &Profile::default()).unwrap()))
    }

    #[test]
    fn native_records_replace_inherit_clear_and_follow_lifecycle() {
        for (_, mut emu) in emulators() {
            emu.process(b"\x1b]7501;state=working:app=deploy\x07\
            \x1b]7501;state=working:id=build:app=cargo\x07\
            \x1b]7501;state=blocked:id=build/test:kind=permission:progress=40:title=VGVzdHM=:msg=QXBwcm92ZT8=\x07\
            \x1b]7501;state=error:id=builder\x07");
            let records = emu.program_statuses().unwrap();
            assert_eq!(records[2].id, "build/test");
            assert_eq!(records[2].app.as_deref(), Some("cargo"));
            assert_eq!(records[2].title.as_deref(), Some("Tests"));
            assert_eq!(records[2].message.as_deref(), Some("Approve?"));
            assert_eq!(records[2].kind, Some(ProgramStatusKind::Permission));
            assert_eq!(records[2].progress, Some(40));
            emu.process(b"\x1b]7501;state=done:id=build\x07");
            assert_eq!(
                emu.program_statuses().unwrap()[2].app.as_deref(),
                Some("deploy")
            );
            emu.process(b"\x1b]7501;state=done:id=build/test\x07");
            let record = &emu.program_statuses().unwrap()[2];
            assert_eq!(record.message, None);
            assert_eq!(record.title, None);
            assert_eq!(record.kind, None);
            assert_eq!(record.progress, None);
            emu.process(b"\x1b[?1049h\x1b[?1049l\x1b[!p");
            assert_eq!(emu.program_statuses().unwrap().len(), 4);
            emu.process(b"\x1b]7501;state=clear:id=build\x07\x1b]133;A\x07");
            assert_eq!(emu.program_statuses().unwrap().len(), 1);
            assert_eq!(emu.program_statuses().unwrap()[0].id, "builder");
            emu.process(b"\x1bc");
            assert!(emu.program_statuses().unwrap().is_empty());
        }
    }

    #[test]
    fn native_queries_reply_once_in_stream_order() {
        for terminator in ["\x07", "\x1b\\"] {
            for (backend, mut emu) in emulators() {
                emu.process(b"\x1b[c");
                let attributes = emu.take_pending_writes();
                emu.process(format!("\x1b[c\x1b]7501;?{terminator}\x1b[c").as_bytes());
                let capabilities = if backend.as_str() == "rio" {
                    "?"
                } else {
                    "?:states=idle,working,done,blocked,error:kinds=permission,question,auth"
                };
                let reply = format!("\x1b]7501;{capabilities}{terminator}");
                assert_eq!(
                    emu.take_pending_writes(),
                    [
                        attributes.as_slice(),
                        reply.as_bytes(),
                        attributes.as_slice()
                    ]
                    .concat()
                );
            }
        }
    }

    #[test]
    fn native_records_evict_the_least_recently_updated_entry() {
        for (_, mut emu) in emulators() {
            for index in 0..256 {
                emu.process(format!("\x1b]7501;state=done:id=task{index}\x07").as_bytes());
            }
            emu.process(b"\x1b]7501;state=error:id=task0\x07\x1b]7501;state=done:id=new\x07");
            let records = emu.program_statuses().unwrap();
            assert_eq!(records.len(), 256);
            assert!(records.iter().any(|record| record.id == "task0"));
            assert!(!records.iter().any(|record| record.id == "task1"));
            assert!(records.iter().any(|record| record.id == "new"));
        }
    }
}
