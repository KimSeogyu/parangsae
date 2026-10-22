//! Gherkin counts and cucumber floors for the production runners.

use std::path::{Path, PathBuf};

pub fn expand_feature_glob(glob: &str) -> Result<Vec<PathBuf>, String> {
    let root = crate::feature_run::workspace_root();
    if glob.ends_with(".feature") && !glob.contains('*') {
        let path = root.join(glob);
        if path.is_file() {
            return Ok(vec![path]);
        }
        return Err(format!("glob {glob} matched no features"));
    }
    let (dir, recursive) = feature_glob_dir(glob)?;
    let mut files = Vec::new();
    collect_features(&root.join(dir), recursive, &mut files)?;
    files.sort();
    if files.is_empty() {
        return Err(format!("glob {glob} matched no features"));
    }
    Ok(files)
}

pub fn enforce_floors(runner: &str, files: &[PathBuf], passed_steps: usize) -> Result<(), String> {
    let (scenarios, steps) = gherkin_counts(files)?;
    let floors = std::fs::read_to_string(
        crate::feature_run::workspace_root().join("regressions/floors.toml"),
    )
    .map_err(|err| format!("read floors: {err}"))?;
    let min_scenarios = floor_usize(&floors, &format!("cucumber.{runner}.scenarios"))?;
    let min_steps = floor_usize(&floors, &format!("cucumber.{runner}.steps"))?;
    let max_skipped = floor_usize(&floors, &format!("cucumber.{runner}.max_skipped"))?;
    if scenarios < min_scenarios || steps < min_steps || passed_steps < min_steps || max_skipped > 0
    {
        return Err(format!(
            "cucumber floor {runner}: scenarios={scenarios} steps={steps} passed={passed_steps} max_skipped={max_skipped}"
        ));
    }
    if steps != passed_steps {
        return Err(format!(
            "executed steps {passed_steps} != feature steps {steps}"
        ));
    }
    Ok(())
}

fn feature_glob_dir(glob: &str) -> Result<(&str, bool), String> {
    if let Some(dir) = glob.strip_suffix("/**/*.feature") {
        Ok((dir, true))
    } else if let Some(dir) = glob.strip_suffix("/*.feature") {
        Ok((dir, false))
    } else {
        Err(format!("unsupported feature glob {glob}"))
    }
}

fn collect_features(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|err| format!("read {}: {err}", dir.display()))?
            .path();
        if path.is_dir() {
            if recursive {
                collect_features(&path, true, out)?;
            }
        } else if path.extension().is_some_and(|ext| ext == "feature") {
            out.push(path);
        }
    }
    Ok(())
}

fn gherkin_counts(files: &[PathBuf]) -> Result<(usize, usize), String> {
    let mut scenarios = 0usize;
    let mut steps = 0usize;
    for path in files {
        let text = std::fs::read_to_string(path)
            .map_err(|err| format!("read {}: {err}", path.display()))?;
        let (file_scenarios, file_steps) = count_feature(&text)
            .map_err(|err| format!("malformed feature {}: {err}", path.display()))?;
        scenarios += file_scenarios;
        steps += file_steps;
    }
    Ok((scenarios, steps))
}

fn count_feature(text: &str) -> Result<(usize, usize), cucumber::gherkin::ParseError> {
    let feature =
        cucumber::gherkin::Feature::parse(text, cucumber::gherkin::GherkinEnv::default())?;
    let mut scenarios = feature.scenarios.len();
    let mut steps = background_steps(&feature.background);
    for scenario in &feature.scenarios {
        steps += scenario.steps.len();
    }
    for rule in &feature.rules {
        scenarios += rule.scenarios.len();
        steps += background_steps(&rule.background);
        for scenario in &rule.scenarios {
            steps += scenario.steps.len();
        }
    }
    Ok((scenarios, steps))
}

fn background_steps(background: &Option<cucumber::gherkin::Background>) -> usize {
    match background {
        Some(background) => background.steps.len(),
        None => 0,
    }
}

fn floor_usize(text: &str, key: &str) -> Result<usize, String> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((raw_key, raw_value)) = line.split_once('=') else {
            return Err(format!("floor line is not key = value: {line}"));
        };
        if raw_key.trim() == key {
            let value: u64 = raw_value
                .trim()
                .parse()
                .map_err(|_| format!("floor {key} is not an integer"))?;
            return usize::try_from(value).map_err(|_| format!("floor {key} does not fit"));
        }
    }
    Err(format!("missing floor {key}"))
}
