// Author: Jeff
// Date: 2026-09-04
// Description: Read VEVENT records from an iCalendar file into events and rules
// Notes: Deliberately narrow. Anything this application cannot represent is named
//        and refused, so an import never lands a weaker schedule than the file states.

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday};
use chrono_tz::Tz;
use thiserror::Error;

use crate::domain::{DomainError, EventFrequency, EventRecurrence, EventTime};

const DATE_FORMAT: &str = "%Y%m%d";
const DATETIME_FORMAT: &str = "%Y%m%dT%H%M%S";
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_EVENTS: usize = 5000;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IcsError {
    #[error("the file is larger than this reader accepts")]
    TooLarge,
    #[error("the file holds more than {MAX_EVENTS} events")]
    TooManyEvents,
    #[error("line {line}: expected a NAME:VALUE property")]
    Malformed { line: usize },
    #[error("line {line}: {name} closes a block that is not open")]
    UnbalancedBlock { line: usize, name: String },
    #[error("the file ends inside an unclosed block")]
    UnclosedBlock,
    #[error("an event is missing its {field}")]
    MissingProperty { field: &'static str },
    #[error("'{value}' is not an iCalendar {kind}")]
    UnreadableValue { kind: &'static str, value: String },
    #[error("'{timezone}' is not a named IANA zone")]
    UnknownTimezone { timezone: String },
    #[error("a timed event needs DTEND; DURATION and zero-length events are not supported")]
    MissingEnd,
    #[error("RRULE part {part} is not supported")]
    UnsupportedRulePart { part: String },
    #[error("a local time in an event does not exist in '{timezone}'")]
    UnrepresentableLocalTime { timezone: String },
    #[error(transparent)]
    Domain(#[from] DomainError),
}

/// One `VEVENT` read from a file, before it is given an identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IcsEvent {
    pub uid: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub time: EventTime,
    pub recurrence: Option<EventRecurrence>,
}

/// Read every `VEVENT` in one iCalendar document.
///
/// `VTIMEZONE` blocks are skipped: their rules describe a zone's daylight-saving
/// history, which `chrono-tz` already knows, and are not event recurrences.
///
/// # Errors
/// Returns an error for an oversized or malformed file, a value this reader
/// cannot read, and an RRULE part this application cannot represent.
pub fn read(text: &str) -> Result<Vec<IcsEvent>, IcsError> {
    if text.len() > MAX_BYTES {
        return Err(IcsError::TooLarge);
    }
    let mut events = Vec::new();
    let mut blocks: Vec<String> = Vec::new();
    let mut current: Option<Draft> = None;

    for (line, property) in unfold(text) {
        let Some((name, parameters, value)) = split_property(&property) else {
            return Err(IcsError::Malformed { line });
        };
        match name.as_str() {
            "BEGIN" => {
                blocks.push(value.clone());
                if value == "VEVENT" && !inside_timezone(&blocks) {
                    current = Some(Draft::default());
                }
            }
            "END" => {
                if blocks.pop().as_deref() != Some(value.as_str()) {
                    return Err(IcsError::UnbalancedBlock { line, name: value });
                }
                if value == "VEVENT"
                    && let Some(draft) = current.take()
                {
                    if events.len() >= MAX_EVENTS {
                        return Err(IcsError::TooManyEvents);
                    }
                    events.push(draft.finish()?);
                }
            }
            // A VTIMEZONE carries its own DTSTART and RRULE; neither is an event's
            _ if inside_timezone(&blocks) => {}
            _ => {
                if let Some(draft) = current.as_mut() {
                    draft.apply(&name, &parameters, &value)?;
                }
            }
        }
    }
    if !blocks.is_empty() {
        return Err(IcsError::UnclosedBlock);
    }
    Ok(events)
}

fn inside_timezone(blocks: &[String]) -> bool {
    blocks.iter().any(|block| block == "VTIMEZONE")
}

/// Join continuation lines, which RFC 5545 marks with a leading space or tab.
fn unfold(text: &str) -> Vec<(usize, String)> {
    let mut properties: Vec<(usize, String)> = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix([' ', '\t'])
            && let Some(last) = properties.last_mut()
        {
            last.1.push_str(rest);
            continue;
        }
        properties.push((index + 1, line.to_owned()));
    }
    properties
}

/// A property's name, its parameters, and its value.
type Property = (String, Vec<(String, String)>, String);

/// Split `NAME;PARAM=VALUE:VALUE` into its three parts.
fn split_property(property: &str) -> Option<Property> {
    let colon = find_value_colon(property)?;
    let (head, value) = property.split_at(colon);
    let value = value[1..].to_owned();
    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_ascii_uppercase();
    if name.is_empty() {
        return None;
    }
    let parameters = parts
        .filter_map(|part| {
            part.split_once('=')
                .map(|(key, value)| (key.trim().to_ascii_uppercase(), value.trim().to_owned()))
        })
        .collect();
    Some((name, parameters, value))
}

/// The colon that ends the property head, ignoring colons inside a quoted parameter.
fn find_value_colon(property: &str) -> Option<usize> {
    let mut quoted = false;
    for (index, character) in property.char_indices() {
        match character {
            '"' => quoted = !quoted,
            ':' if !quoted => return Some(index),
            _ => {}
        }
    }
    None
}

/// Undo the text escaping RFC 5545 applies to SUMMARY and DESCRIPTION.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match characters.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(escaped) => out.push(escaped),
            None => out.push('\\'),
        }
    }
    out
}

#[derive(Debug, Default)]
struct Draft {
    uid: Option<String>,
    title: Option<String>,
    description: Option<String>,
    start: Option<Stamp>,
    end: Option<Stamp>,
    rule: Option<EventRecurrence>,
}

/// A DTSTART or DTEND value, before the pair is resolved into an `EventTime`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stamp {
    Date(NaiveDate),
    Local { at: NaiveDateTime, timezone: String },
}

impl Draft {
    fn apply(
        &mut self,
        name: &str,
        parameters: &[(String, String)],
        value: &str,
    ) -> Result<(), IcsError> {
        match name {
            "UID" => self.uid = Some(value.trim().to_owned()),
            "SUMMARY" => self.title = Some(unescape(value)),
            "DESCRIPTION" => self.description = Some(unescape(value)),
            "DTSTART" => self.start = Some(read_stamp(parameters, value)?),
            "DTEND" => self.end = Some(read_stamp(parameters, value)?),
            "RRULE" => self.rule = Some(read_rule(value)?),
            _ => {}
        }
        Ok(())
    }

    fn finish(self) -> Result<IcsEvent, IcsError> {
        let title = self
            .title
            .filter(|title| !title.trim().is_empty())
            .ok_or(IcsError::MissingProperty { field: "SUMMARY" })?;
        let start = self
            .start
            .ok_or(IcsError::MissingProperty { field: "DTSTART" })?;
        let time = match (start, self.end) {
            // An all-day event with no DTEND covers exactly one day
            (Stamp::Date(start), None) => EventTime::all_day(
                start,
                start.succ_opt().ok_or(DomainError::InvalidAllDayRange)?,
            )?,
            (Stamp::Date(start), Some(Stamp::Date(end))) => EventTime::all_day(start, end)?,
            (Stamp::Local { .. }, None) => return Err(IcsError::MissingEnd),
            (
                Stamp::Local {
                    at: start,
                    timezone,
                },
                Some(Stamp::Local {
                    at: end,
                    timezone: end_timezone,
                }),
            ) => {
                let start = zoned(start, &timezone)?;
                let end = zoned(end, &end_timezone)?;
                EventTime::timed(start, end, timezone)?
            }
            _ => {
                return Err(IcsError::UnreadableValue {
                    kind: "date range",
                    value: "DTSTART and DTEND disagree about being all-day".to_owned(),
                });
            }
        };
        Ok(IcsEvent {
            uid: self.uid.filter(|uid| !uid.is_empty()),
            title,
            description: self.description.filter(|text| !text.trim().is_empty()),
            time,
            recurrence: self.rule,
        })
    }
}

/// Anchor a local time in its zone, refusing one that does not exist there.
fn zoned(
    at: NaiveDateTime,
    timezone: &str,
) -> Result<chrono::DateTime<chrono::FixedOffset>, IcsError> {
    let zone = timezone
        .parse::<Tz>()
        .map_err(|_| IcsError::UnknownTimezone {
            timezone: timezone.to_owned(),
        })?;
    zone.from_local_datetime(&at)
        .single()
        .map(|value| value.fixed_offset())
        .ok_or_else(|| IcsError::UnrepresentableLocalTime {
            timezone: timezone.to_owned(),
        })
}

fn read_stamp(parameters: &[(String, String)], value: &str) -> Result<Stamp, IcsError> {
    let parameter = |wanted: &str| {
        parameters
            .iter()
            .find(|(key, _)| key == wanted)
            .map(|(_, value)| value.as_str())
    };
    let value = value.trim();
    if parameter("VALUE") == Some("DATE") {
        return NaiveDate::parse_from_str(value, DATE_FORMAT)
            .map(Stamp::Date)
            .map_err(|_| IcsError::UnreadableValue {
                kind: "date",
                value: value.to_owned(),
            });
    }
    // A trailing Z is UTC; every other form carries its zone in TZID
    let (naive, timezone) = if let Some(stripped) = value.strip_suffix('Z') {
        (stripped, "UTC".to_owned())
    } else {
        (
            value,
            parameter("TZID")
                .ok_or(IcsError::MissingProperty { field: "TZID" })?
                .to_owned(),
        )
    };
    let at = NaiveDateTime::parse_from_str(naive, DATETIME_FORMAT).or_else(|_| {
        NaiveDate::parse_from_str(naive, DATE_FORMAT)
            .map(|date| date.and_time(NaiveTime::MIN))
            .map_err(|_| IcsError::UnreadableValue {
                kind: "date-time",
                value: naive.to_owned(),
            })
    })?;
    Ok(Stamp::Local { at, timezone })
}

/// Read an RRULE, naming any part this application cannot represent.
fn read_rule(value: &str) -> Result<EventRecurrence, IcsError> {
    let mut frequency = None;
    let mut interval = 1_u32;
    let mut count = None;
    let mut until = None;
    let mut by_weekday = Vec::new();

    for part in value.split(';').filter(|part| !part.trim().is_empty()) {
        let (key, raw) = part
            .split_once('=')
            .ok_or_else(|| IcsError::UnreadableValue {
                kind: "RRULE part",
                value: part.to_owned(),
            })?;
        let key = key.trim().to_ascii_uppercase();
        let raw = raw.trim();
        match key.as_str() {
            "FREQ" => {
                frequency = Some(match raw.to_ascii_uppercase().as_str() {
                    "DAILY" => EventFrequency::Daily,
                    "WEEKLY" => EventFrequency::Weekly,
                    "MONTHLY" => EventFrequency::Monthly,
                    other => {
                        return Err(IcsError::UnsupportedRulePart {
                            part: format!("FREQ={other}"),
                        });
                    }
                });
            }
            "INTERVAL" => {
                interval = raw.parse().map_err(|_| IcsError::UnreadableValue {
                    kind: "RRULE INTERVAL",
                    value: raw.to_owned(),
                })?;
            }
            "COUNT" => {
                count = Some(raw.parse().map_err(|_| IcsError::UnreadableValue {
                    kind: "RRULE COUNT",
                    value: raw.to_owned(),
                })?);
            }
            "UNTIL" => {
                let date = raw.split('T').next().unwrap_or(raw);
                until = Some(NaiveDate::parse_from_str(date, DATE_FORMAT).map_err(|_| {
                    IcsError::UnreadableValue {
                        kind: "RRULE UNTIL",
                        value: raw.to_owned(),
                    }
                })?);
            }
            "BYDAY" => {
                for day in raw.split(',').filter(|day| !day.trim().is_empty()) {
                    by_weekday.push(read_weekday(day.trim())?);
                }
            }
            "WKST" => {
                // Only affects weeks this reader does not split on, and accepting
                // it silently would change which days a BYWEEKNO rule lands on.
                return Err(IcsError::UnsupportedRulePart { part: key });
            }
            other => {
                return Err(IcsError::UnsupportedRulePart {
                    part: other.to_owned(),
                });
            }
        }
    }
    let frequency = frequency.ok_or(IcsError::MissingProperty { field: "FREQ" })?;
    Ok(EventRecurrence::new(
        frequency, interval, count, until, by_weekday,
    )?)
}

/// Read a BYDAY value, refusing the ordinal form such as `1SU`.
fn read_weekday(value: &str) -> Result<Weekday, IcsError> {
    let day = match value.to_ascii_uppercase().as_str() {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => {
            return Err(IcsError::UnsupportedRulePart {
                part: format!("BYDAY={value}"),
            });
        }
    };
    Ok(day)
}
