use anyhow::{bail, Result};
use ghostty_vt::terminal::{
    ProgramStatus as Report, ProgramStatusKind as Kind, ProgramStatusState as State,
};

use crate::api::{ProgramState, ProgramStatus, ProgramStatusKind};

/// Ghostty validates and decodes reports. Retain owned records in update order.
#[derive(Default)]
pub(super) struct ProgramStatuses {
    records: Vec<ProgramStatus>,
}

impl ProgramStatuses {
    pub fn report(&mut self, report: Report<'_>) -> Result<()> {
        let id = std::str::from_utf8(report.id())?.to_owned();
        let state = match report.state()? {
            State::Clear => {
                self.records.retain(|record| {
                    !id.is_empty()
                        && record.id != id
                        && !record
                            .id
                            .strip_prefix(&id)
                            .is_some_and(|suffix| suffix.starts_with('/'))
                });
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
        self.records.retain(|old| old.id != record.id);
        if self.records.len() == 256 {
            self.records.remove(0);
        }
        self.records.push(record);
        Ok(())
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

fn text(bytes: &[u8]) -> Result<Option<String>> {
    Ok((!bytes.is_empty())
        .then(|| std::str::from_utf8(bytes).map(str::to_owned))
        .transpose()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{profile::Profile, terminal::emu::Emulator, terminal::ghostty::GhosttyEmu};

    #[test]
    fn native_records_replace_inherit_clear_and_follow_lifecycle() {
        let mut emu = GhosttyEmu::new(80, 24, &Profile::default()).unwrap();
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

    #[test]
    fn native_queries_reply_once_in_stream_order() {
        for terminator in ["\x07", "\x1b\\"] {
            let mut emu = GhosttyEmu::new(80, 24, &Profile::default()).unwrap();
            emu.process(b"\x1b[c");
            let attributes = emu.take_pending_writes();
            emu.process(format!("\x1b[c\x1b]7501;?{terminator}\x1b[c").as_bytes());
            let reply = format!(
                "\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth{terminator}"
            );
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

    #[test]
    fn native_records_evict_the_least_recently_updated_entry() {
        let mut emu = GhosttyEmu::new(80, 24, &Profile::default()).unwrap();
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
