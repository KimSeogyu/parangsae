//! Filename allow-regex. Names that do not match are quarantined, not parsed.

use regex::Regex;

use super::VisionDataset;
use crate::civil::{CivilDay, YearMonth};
use crate::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cadence {
    Daily,
    Monthly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveName {
    pub cadence: Cadence,
    pub day: Option<CivilDay>,
    pub month: YearMonth,
    pub file_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Classified {
    Archive(ArchiveName),
    Checksum { zip_file_name: String },
    Quarantine { reason: &'static str },
}

pub fn classify(
    dataset: VisionDataset,
    symbol: &str,
    file_name: &str,
) -> Result<Classified, Error> {
    if let Some(zip_file_name) = file_name.strip_suffix(".CHECKSUM") {
        return match classify(dataset, symbol, zip_file_name)? {
            Classified::Archive(_) => Ok(Classified::Checksum {
                zip_file_name: zip_file_name.to_owned(),
            }),
            Classified::Quarantine { reason } => Ok(Classified::Quarantine { reason }),
            Classified::Checksum { .. } => Ok(Classified::Quarantine { reason: "filename" }),
        };
    }
    let pattern = allow_pattern(dataset, symbol)?;
    let Some(caps) = pattern.captures(file_name) else {
        return Ok(Classified::Quarantine { reason: "filename" });
    };
    let year: i32 = caps
        .get(1)
        .and_then(|m| m.as_str().parse().ok())
        .ok_or_else(|| Error::check("filename", "year"))?;
    let month: u8 = caps
        .get(2)
        .and_then(|m| m.as_str().parse().ok())
        .ok_or_else(|| Error::check("filename", "month"))?;
    let Some(year_month) = YearMonth::new(year, month) else {
        return Ok(Classified::Quarantine {
            reason: "invalid_date",
        });
    };
    let day = match caps.get(3) {
        Some(matched) => {
            let day_num: u8 = matched
                .as_str()
                .parse()
                .map_err(|_| Error::check("filename", "day"))?;
            match CivilDay::new(year, month, day_num) {
                Some(day) => Some(day),
                None => {
                    return Ok(Classified::Quarantine {
                        reason: "invalid_date",
                    })
                }
            }
        }
        None => None,
    };
    let cadence = if day.is_some() {
        Cadence::Daily
    } else {
        Cadence::Monthly
    };
    if cadence == Cadence::Monthly && !dataset.allows_monthly() {
        return Ok(Classified::Quarantine { reason: "filename" });
    }
    if cadence == Cadence::Daily && matches!(dataset, VisionDataset::FundingRate) {
        return Ok(Classified::Quarantine { reason: "filename" });
    }
    Ok(Classified::Archive(ArchiveName {
        cadence,
        day,
        month: year_month,
        file_name: file_name.to_owned(),
    }))
}

fn allow_pattern(dataset: VisionDataset, symbol: &str) -> Result<Regex, Error> {
    let sym = regex::escape(symbol);
    let token = regex::escape(dataset.file_token());
    let body = match dataset {
        VisionDataset::FundingRate => format!(r"^{sym}-{token}-(\d{{4}})-(\d{{2}})\.zip$"),
        VisionDataset::Metrics | VisionDataset::BookDepth => {
            format!(r"^{sym}-{token}-(\d{{4}})-(\d{{2}})-(\d{{2}})\.zip$")
        }
        VisionDataset::UmKline
        | VisionDataset::MarkPriceKline
        | VisionDataset::IndexPriceKline
        | VisionDataset::PremiumIndexKline
        | VisionDataset::SpotKline => {
            format!(r"^{sym}-{token}-(\d{{4}})-(\d{{2}})(?:-(\d{{2}}))?\.zip$")
        }
    };
    Regex::new(&body).map_err(|err| Error::check("filename", err.to_string()))
}

pub fn basename(key: &str) -> &str {
    key.rsplit('/').next().unwrap_or(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_files_are_quarantined_and_daily_names_are_not() -> Result<(), String> {
        let part = classify(VisionDataset::UmKline, "BTCUSDT", "part-00000-c000.zip")
            .map_err(|err| err.to_string())?;
        assert!(matches!(
            part,
            Classified::Quarantine { reason: "filename" }
        ));
        let daily = classify(
            VisionDataset::UmKline,
            "BTCUSDT",
            "BTCUSDT-1m-2021-06-01.zip",
        )
        .map_err(|err| err.to_string())?;
        assert!(matches!(daily, Classified::Archive(_)));
        let checksum = classify(
            VisionDataset::UmKline,
            "BTCUSDT",
            "BTCUSDT-1m-2021-06-01.zip.CHECKSUM",
        )
        .map_err(|err| err.to_string())?;
        assert!(matches!(checksum, Classified::Checksum { .. }));
        Ok(())
    }

    #[test]
    fn february_31_is_quarantined() -> Result<(), String> {
        let classified = classify(
            VisionDataset::Metrics,
            "BTCUSDT",
            "BTCUSDT-metrics-2026-02-31.zip",
        )
        .map_err(|err| err.to_string())?;
        assert!(matches!(
            classified,
            Classified::Quarantine {
                reason: "invalid_date"
            }
        ));
        Ok(())
    }
}
