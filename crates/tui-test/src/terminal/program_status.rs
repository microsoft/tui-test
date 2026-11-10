use base64::engine::{general_purpose::GeneralPurpose, DecodePaddingMode, GeneralPurposeConfig};
use base64::Engine as _;

use crate::api::{valid_program_status_id, ProgramState, ProgramStatus, ProgramStatusKind};

const BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// Records stay in update order, which bounds eviction work to 256 entries.
#[derive(Default)]
pub(super) struct ProgramStatuses {
    records: Vec<ProgramStatus>,
}

impl ProgramStatuses {
    pub fn report(&mut self, body: &[u8]) {
        let Some(report) = parse(body) else { return };
        match report {
            Report::Clear(id) => self.records.retain(|record| {
                !id.is_empty()
                    && record.id != id
                    && !record
                        .id
                        .strip_prefix(&id)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            }),
            Report::Status(record) => {
                self.records.retain(|old| old.id != record.id);
                if self.records.len() == 256 {
                    self.records.remove(0);
                }
                self.records.push(record);
            }
        }
    }

    pub fn clear(&mut self) {
        self.records.clear();
    }

    pub fn finish_process(&mut self) {
        self.records.retain(|record| {
            !matches!(record.state, ProgramState::Working | ProgramState::Blocked)
        });
    }

    pub fn get(&self, id: &str) -> Option<ProgramStatus> {
        let mut record = self.records.iter().find(|record| record.id == id)?.clone();
        let mut parent = id;
        while record.app.is_none() && !parent.is_empty() {
            parent = parent.rsplit_once('/').map_or("", |(parent, _)| parent);
            record.app = self
                .records
                .iter()
                .find(|record| record.id == parent)
                .and_then(|record| record.app.clone());
        }
        Some(record)
    }

    pub fn all(&self) -> Vec<ProgramStatus> {
        let mut records: Vec<_> = self
            .records
            .iter()
            .filter_map(|record| self.get(&record.id))
            .collect();
        records.sort_by(|left, right| left.id.cmp(&right.id));
        records
    }
}

enum Report {
    Clear(String),
    Status(ProgramStatus),
}

fn text(value: &[u8], encoded_limit: usize, decoded_limit: usize) -> Option<String> {
    if value.len() > encoded_limit {
        return None;
    }
    let decoded = BASE64.decode(value).ok()?;
    if decoded.len() > decoded_limit {
        return None;
    }
    let decoded = String::from_utf8(decoded).ok()?;
    (!decoded.chars().any(char::is_control)).then_some(decoded)
}

fn parse(body: &[u8]) -> Option<Report> {
    let mut id = None;
    let mut state = None;
    let mut kind = None;
    let mut progress = None;
    let mut app = None;
    let mut title = None;
    let mut message = None;
    for pair in body.split(|byte| *byte == b':') {
        let equal = match pair.iter().position(|byte| *byte == b'=') {
            Some(equal) => equal,
            None => continue,
        };
        let key = pair[..equal].trim_ascii();
        let value = pair[equal + 1..].trim_ascii();
        if key.len() > 16 {
            return None;
        }

        // Check every known field before duplicates can replace it.
        match key {
            b"id" if value.len() > 128 => return None,
            b"app" if value.len() > 32 => return None,
            b"title" if value.len() > 256 => return None,
            b"msg" if value.len() > 2732 => return None,
            _ => {}
        }
        if key == b"id" {
            if value.split(|byte| *byte == b'/').count() > 8
                || value
                    .split(|byte| *byte == b'/')
                    .any(|segment| segment.len() > 32)
            {
                return None;
            }
            // An invalid id must never fall back to the root record.
            id = Some(value);
            continue;
        }
        if key.is_empty()
            || !key.iter().all(u8::is_ascii_lowercase)
            || !value
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.,+/=-".contains(byte))
        {
            continue;
        }
        match key {
            b"state" => state = Some(value),
            b"kind" => {
                kind = match value {
                    b"permission" => Some(ProgramStatusKind::Permission),
                    b"question" => Some(ProgramStatusKind::Question),
                    b"auth" => Some(ProgramStatusKind::Auth),
                    _ => None,
                }
            }
            b"progress" => progress = super::reports::percentage(value),
            b"app" => {
                app = (!value.is_empty()
                    && value
                        .iter()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_.+-".contains(byte)))
                .then(|| String::from_utf8(value.to_vec()).expect("ASCII app"));
            }
            b"title" => title = Some(text(value, 256, 192)?),
            b"msg" => message = Some(text(value, 2732, 2048)?),
            _ => {}
        }
    }
    let id = match id {
        None => String::new(),
        Some(value) => {
            let value = std::str::from_utf8(value).ok()?;
            if value.is_empty() || !valid_program_status_id(value) {
                return None;
            }
            value.to_string()
        }
    };
    let state = match state? {
        b"clear" => return Some(Report::Clear(id)),
        b"idle" => ProgramState::Idle,
        b"working" => ProgramState::Working,
        b"done" => ProgramState::Done,
        b"blocked" => ProgramState::Blocked,
        b"error" => ProgramState::Error,
        _ => return None,
    };
    Some(Report::Status(ProgramStatus {
        id,
        state,
        app,
        title,
        message,
        kind: (state == ProgramState::Blocked).then_some(kind).flatten(),
        progress: matches!(state, ProgramState::Working | ProgramState::Blocked)
            .then_some(progress)
            .flatten(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_replace_fields_and_inherit_app_from_the_nearest_live_ancestor() {
        let mut statuses = ProgramStatuses::default();
        statuses.report(b"state=working:app=deploy");
        statuses.report(b"state=working:id=build:app=cargo");
        statuses.report(b"state=blocked:id=build/test:kind=permission:progress=40:title=VGVzdHM=:msg=QXBwcm92ZT8");
        let record = statuses.get("build/test").unwrap();
        assert_eq!(record.app.as_deref(), Some("cargo"));
        assert_eq!(record.title.as_deref(), Some("Tests"));
        assert_eq!(record.message.as_deref(), Some("Approve?"));
        assert_eq!(record.kind, Some(ProgramStatusKind::Permission));
        assert_eq!(record.progress, Some(40));
        statuses.report(b"state=done:id=build");
        assert_eq!(
            statuses.get("build/test").unwrap().app.as_deref(),
            Some("deploy")
        );
        statuses.report(b"state=done:id=build/test:kind=auth:progress=100");
        let record = statuses.get("build/test").unwrap();
        assert_eq!(record.message, None);
        assert_eq!(record.title, None);
        assert_eq!(record.progress, None);
        assert_eq!(record.kind, None);
    }

    #[test]
    fn clear_respects_path_boundaries_and_lifecycle_keeps_completed_records() {
        let mut statuses = ProgramStatuses::default();
        for body in [
            "state=idle",
            "state=working:id=build",
            "state=done:id=build/test",
            "state=error:id=builder",
            "state=blocked:id=auth",
        ] {
            statuses.report(body.as_bytes());
        }
        statuses.finish_process();
        assert_eq!(
            statuses
                .all()
                .iter()
                .map(|record| record.id.as_str())
                .collect::<Vec<_>>(),
            ["", "build/test", "builder"]
        );
        statuses.report(b"state=clear:id=build");
        assert!(statuses.get("build/test").is_none());
        assert!(statuses.get("builder").is_some());
        statuses.report(b"state=clear");
        assert!(statuses.all().is_empty());
    }

    #[test]
    fn duplicate_fields_and_malformed_pairs_follow_protocol_rules() {
        let mut statuses = ProgramStatuses::default();
        statuses.report(b"state=error:state = blocked :bad pair:unknown=value:kind=auth:progress=no:app=bad/name:msg=SGk");
        let record = statuses.get("").unwrap();
        assert_eq!(record.state, ProgramState::Blocked);
        assert_eq!(record.kind, Some(ProgramStatusKind::Auth));
        assert_eq!(record.progress, None);
        assert_eq!(record.app, None);
        assert_eq!(record.message.as_deref(), Some("Hi"));
        statuses.report(b"state=working:id=bad//path:id=good:progress=101:kind=future");
        assert_eq!(statuses.get("good").unwrap().progress, None);
        assert_eq!(statuses.get("good").unwrap().kind, None);
    }

    #[test]
    fn invalid_reports_and_every_field_limit_are_atomic() {
        let mut statuses = ProgramStatuses::default();
        statuses.report(b"state=done:app=saved");
        let before = statuses.all();
        let invalid = vec![
            "app=missing-state".into(),
            "state=future".into(),
            "state=working:id=".into(),
            "state=working:id=bad//path".into(),
            "state=working:id=bad!".into(),
            "state=working:id=/root".into(),
            "state=working:msg=/w==".into(),
            "state=working:msg=a:msg=SGk=".into(),
            format!("state=working:msg={}", BASE64.encode("line\nbreak")),
            format!("state=working:title={}", BASE64.encode("\u{0080}")),
            format!("state=working:msg={}", BASE64.encode("x".repeat(2049))),
            format!("state=working:title={}", BASE64.encode("x".repeat(193))),
            format!("state=working:msg={}:msg=SGk=", "x".repeat(2733)),
            format!("state=working:app={}:app=ok", "a".repeat(33)),
            format!("state=working:id={}", "a".repeat(33)),
            format!("state=working:id={}", ["a"; 9].join("/")),
            format!("state=working:id={}", vec!["a".repeat(25); 5].join("/")),
            "state=working:unknownkeytoolongx=yes".into(),
        ];
        for report in invalid {
            statuses.report(report.as_bytes());
            assert_eq!(statuses.all(), before, "{report}");
        }
        statuses.report(
            format!(
                "state=blocked:msg={}:title={}",
                BASE64.encode("m".repeat(2048)),
                BASE64.encode("t".repeat(192))
            )
            .as_bytes(),
        );
        let record = statuses.get("").unwrap();
        assert_eq!(record.message.unwrap().len(), 2048);
        assert_eq!(record.title.unwrap().len(), 192);
    }

    #[test]
    fn capacity_evicts_the_least_recently_updated_record() {
        let mut statuses = ProgramStatuses::default();
        for index in 0..256 {
            statuses.report(format!("state=done:id=task{index}").as_bytes());
        }
        statuses.report(b"state=error:id=task0");
        statuses.report(b"state=done:id=new");
        assert_eq!(statuses.all().len(), 256);
        assert!(statuses.get("task0").is_some());
        assert!(statuses.get("task1").is_none());
        assert!(statuses.get("new").is_some());
    }
}
