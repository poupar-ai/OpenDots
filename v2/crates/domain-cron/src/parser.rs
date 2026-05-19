//! Parser de cron expression 6-campo + computação de `next_run`.
//!
//! Formato:
//!
//! ```text
//! sec  min  hour  dom  mon  dow
//! 0-59 0-59 0-23  1-31 1-12 0-6 (0=domingo)
//! ```
//!
//! Sintaxe por campo: `*`, `N`, `N-M`, `N,M,...`, `*/N`, `N-M/S`.
//! Atalhos: `@yearly`/`@annually`, `@monthly`, `@weekly`, `@daily`/`@midnight`,
//! `@hourly`.
//!
//! `next_run(after_ms)` retorna o **próximo** segundo estritamente após
//! `after_ms` em que a expr casa todos os campos. Sem timezone explícito
//! — usa UTC (semântica simples e consistente entre boxes).

use std::collections::BTreeSet;
use time::{Date, Duration, Month, OffsetDateTime, Time, Weekday};

#[derive(Debug, thiserror::Error)]
pub enum CronError {
    #[error("expr vazia")]
    Empty,
    #[error("número de campos inválido (esperado 6 ou shortcut, obtido {0})")]
    WrongFieldCount(usize),
    #[error("campo {field} ({raw:?}): {reason}")]
    BadField {
        field: &'static str,
        raw: String,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cron {
    pub seconds: BTreeSet<u8>,
    pub minutes: BTreeSet<u8>,
    pub hours: BTreeSet<u8>,
    pub doms: BTreeSet<u8>,   // 1-31
    pub months: BTreeSet<u8>, // 1-12
    pub dows: BTreeSet<u8>,   // 0-6, 0=Sunday
}

const FIELD_NAMES: [&str; 6] = ["sec", "min", "hour", "dom", "mon", "dow"];
const FIELD_RANGES: [(u8, u8); 6] = [(0, 59), (0, 59), (0, 23), (1, 31), (1, 12), (0, 6)];

pub fn parse(expr: &str) -> Result<Cron, CronError> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Err(CronError::Empty);
    }
    let expanded = match trimmed {
        "@yearly" | "@annually" => "0 0 0 1 1 *",
        "@monthly" => "0 0 0 1 * *",
        "@weekly" => "0 0 0 * * 0",
        "@daily" | "@midnight" => "0 0 0 * * *",
        "@hourly" => "0 0 * * * *",
        s => s,
    };
    let parts: Vec<&str> = expanded.split_whitespace().collect();
    if parts.len() != 6 {
        return Err(CronError::WrongFieldCount(parts.len()));
    }
    let mut sets: [BTreeSet<u8>; 6] = Default::default();
    for (i, part) in parts.iter().enumerate() {
        sets[i] = parse_field(part, FIELD_RANGES[i], FIELD_NAMES[i])?;
    }
    let [seconds, minutes, hours, doms, months, dows] = sets;
    Ok(Cron {
        seconds,
        minutes,
        hours,
        doms,
        months,
        dows,
    })
}

fn parse_field(
    raw: &str,
    (lo, hi): (u8, u8),
    name: &'static str,
) -> Result<BTreeSet<u8>, CronError> {
    let mut out: BTreeSet<u8> = BTreeSet::new();
    for token in raw.split(',') {
        let (range, step) = match token.split_once('/') {
            Some((r, s)) => (
                r,
                s.parse::<u8>().map_err(|_| CronError::BadField {
                    field: name,
                    raw: raw.to_string(),
                    reason: format!("step inválido em {token:?}"),
                })?,
            ),
            None => (token, 1),
        };
        if step == 0 {
            return Err(CronError::BadField {
                field: name,
                raw: raw.to_string(),
                reason: "step não pode ser 0".into(),
            });
        }
        let (start, end) = if range == "*" {
            (lo, hi)
        } else if let Some((a, b)) = range.split_once('-') {
            let a: u8 = a.parse().map_err(|_| CronError::BadField {
                field: name,
                raw: raw.to_string(),
                reason: format!("range start inválido em {range:?}"),
            })?;
            let b: u8 = b.parse().map_err(|_| CronError::BadField {
                field: name,
                raw: raw.to_string(),
                reason: format!("range end inválido em {range:?}"),
            })?;
            (a, b)
        } else {
            let n: u8 = range.parse().map_err(|_| CronError::BadField {
                field: name,
                raw: raw.to_string(),
                reason: format!("valor inválido em {range:?}"),
            })?;
            (n, n)
        };
        if start < lo || end > hi || start > end {
            return Err(CronError::BadField {
                field: name,
                raw: raw.to_string(),
                reason: format!("range {start}-{end} fora de [{lo},{hi}]"),
            });
        }
        let mut v = start;
        while v <= end {
            out.insert(v);
            v = match v.checked_add(step) {
                Some(n) => n,
                None => break,
            };
        }
    }
    if out.is_empty() {
        return Err(CronError::BadField {
            field: name,
            raw: raw.to_string(),
            reason: "set vazio após parse".into(),
        });
    }
    Ok(out)
}

/// Computa o próximo segundo (em ms desde epoch) onde a expr casa,
/// estritamente após `after_ms`. Limita busca a ~4 anos pra cobrir
/// edge cases (Feb 29, expr satisfazível só uma vez ao ano, etc).
pub fn next_run(cron: &Cron, after_ms: u64) -> Option<u64> {
    let after = OffsetDateTime::from_unix_timestamp((after_ms / 1000) as i64).ok()?;
    let mut t = after + Duration::seconds(1);
    let limit = after + Duration::days(4 * 365 + 1);
    while t <= limit {
        if !cron.months.contains(&u8::from(t.month())) {
            t = bump_month(t)?;
            continue;
        }
        let dom_ok = cron.doms.contains(&t.day());
        let dow_ok = cron.dows.contains(&weekday_num(t.weekday()));
        if !(dom_ok && dow_ok) {
            t = bump_day(t)?;
            continue;
        }
        if !cron.hours.contains(&t.hour()) {
            t = bump_hour(t)?;
            continue;
        }
        if !cron.minutes.contains(&t.minute()) {
            t = bump_minute(t)?;
            continue;
        }
        if !cron.seconds.contains(&t.second()) {
            t = t + Duration::seconds(1);
            continue;
        }
        return Some((t.unix_timestamp() as u64) * 1000);
    }
    None
}

fn weekday_num(w: Weekday) -> u8 {
    match w {
        Weekday::Sunday => 0,
        Weekday::Monday => 1,
        Weekday::Tuesday => 2,
        Weekday::Wednesday => 3,
        Weekday::Thursday => 4,
        Weekday::Friday => 5,
        Weekday::Saturday => 6,
    }
}

fn bump_minute(t: OffsetDateTime) -> Option<OffsetDateTime> {
    Some(t.replace_second(0).ok()? + Duration::minutes(1))
}

fn bump_hour(t: OffsetDateTime) -> Option<OffsetDateTime> {
    let zeroed = t.replace_second(0).ok()?.replace_minute(0).ok()?;
    Some(zeroed + Duration::hours(1))
}

fn bump_day(t: OffsetDateTime) -> Option<OffsetDateTime> {
    Some(t.replace_time(Time::MIDNIGHT) + Duration::days(1))
}

fn bump_month(t: OffsetDateTime) -> Option<OffsetDateTime> {
    let year = t.year();
    let m = u8::from(t.month());
    let (new_y, new_m) = if m == 12 { (year + 1, 1) } else { (year, m + 1) };
    let month = Month::try_from(new_m).ok()?;
    let date = Date::from_calendar_date(new_y, month, 1).ok()?;
    Some(date.with_time(Time::MIDNIGHT).assume_utc())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(y: i32, m: u8, d: u8, h: u8, mi: u8, s: u8) -> u64 {
        let month = Month::try_from(m).unwrap();
        let date = Date::from_calendar_date(y, month, d).unwrap();
        let time = Time::from_hms(h, mi, s).unwrap();
        (date.with_time(time).assume_utc().unix_timestamp() as u64) * 1000
    }

    #[test]
    fn parse_every_minute() {
        let c = parse("0 * * * * *").unwrap();
        assert_eq!(c.seconds.len(), 1);
        assert_eq!(c.minutes.len(), 60);
    }

    #[test]
    fn parse_every_5_seconds() {
        let c = parse("*/5 * * * * *").unwrap();
        assert_eq!(c.seconds.len(), 12);
        assert!(c.seconds.contains(&0));
        assert!(c.seconds.contains(&55));
    }

    #[test]
    fn parse_list_and_range() {
        let c = parse("0 0,15,30,45 9-17 * * 1-5").unwrap();
        assert!(c.minutes.contains(&0) && c.minutes.contains(&45));
        assert_eq!(c.hours.len(), 9);
        assert_eq!(c.dows.len(), 5);
    }

    #[test]
    fn parse_shortcuts() {
        assert_eq!(parse("@hourly").unwrap(), parse("0 0 * * * *").unwrap());
        assert_eq!(parse("@daily").unwrap(), parse("0 0 0 * * *").unwrap());
    }

    #[test]
    fn parse_invalid_fields_rejected() {
        assert!(parse("").is_err());
        assert!(parse("* * *").is_err());
        assert!(parse("0 0 0 0 1 *").is_err());
        assert!(parse("0 60 * * * *").is_err());
    }

    #[test]
    fn next_run_every_minute() {
        let c = parse("0 * * * * *").unwrap();
        let after = ts(2026, 5, 19, 12, 0, 0);
        assert_eq!(next_run(&c, after).unwrap(), ts(2026, 5, 19, 12, 1, 0));
    }

    #[test]
    fn next_run_every_5_seconds() {
        let c = parse("*/5 * * * * *").unwrap();
        let after = ts(2026, 5, 19, 12, 0, 3);
        assert_eq!(next_run(&c, after).unwrap(), ts(2026, 5, 19, 12, 0, 5));
    }

    #[test]
    fn next_run_crosses_month() {
        let c = parse("0 0 0 1 1 *").unwrap();
        let after = ts(2026, 12, 31, 23, 59, 59);
        assert_eq!(next_run(&c, after).unwrap(), ts(2027, 1, 1, 0, 0, 0));
    }
}
