use clap::{Arg, ArgAction, Command};

use crate::{Error, Result};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliCommand {
    pub dir: Option<String>,
    pub enable_rpc: bool,
    pub rpc_listen_port: u16,
    pub rpc_secret: Option<String>,
    pub continue_download: bool,
    pub split: Option<u16>,
    pub input_file: Option<String>,
    pub save_session: Option<String>,
    pub uris: Vec<String>,
    pub dispositions: Vec<OptionDisposition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OptionDisposition {
    Pruned { name: String },
    UnsupportedPhaseOne { name: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputTask {
    pub uris: Vec<String>,
    pub options: Vec<(String, String)>,
}

pub fn parse_cli<I, T>(args: I) -> Result<CliCommand>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let matches = command()
        .try_get_matches_from(args)
        .map_err(|error| Error::CliParse(error.to_string()))?;

    Ok(CliCommand {
        dir: matches.get_one::<String>("dir").cloned(),
        enable_rpc: get_bool(&matches, "enable-rpc"),
        rpc_listen_port: matches
            .get_one::<String>("rpc-listen-port")
            .map(|port| port.parse::<u16>())
            .transpose()
            .map_err(|error| Error::CliParse(error.to_string()))?
            .unwrap_or(6800),
        rpc_secret: matches.get_one::<String>("rpc-secret").cloned(),
        continue_download: get_bool(&matches, "continue"),
        split: matches
            .get_one::<String>("split")
            .map(|split| split.parse::<u16>())
            .transpose()
            .map_err(|error| Error::CliParse(error.to_string()))?,
        input_file: matches.get_one::<String>("input-file").cloned(),
        save_session: matches.get_one::<String>("save-session").cloned(),
        uris: matches
            .get_many::<String>("uris")
            .into_iter()
            .flatten()
            .cloned()
            .collect(),
        dispositions: collect_dispositions(&matches),
    })
}

pub fn parse_config_text(text: &str) -> Result<CliCommand> {
    let mut args = vec!["raria".to_string()];
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| Error::CliParse(format!("invalid config line: {line}")))?;
        args.push(format!("--{}={}", key.trim(), value.trim()));
    }
    parse_cli(args)
}

pub fn parse_input_file_text(text: &str) -> Result<Vec<InputTask>> {
    let mut tasks = Vec::new();
    let mut current: Option<InputTask> = None;

    for line in text.lines() {
        if line.trim().is_empty() {
            flush_task(&mut tasks, &mut current);
            continue;
        }

        if line.starts_with(' ') || line.starts_with('\t') {
            let task = current
                .as_mut()
                .ok_or_else(|| Error::CliParse("option line before URI".into()))?;
            let option = line.trim();
            let (key, value) = option
                .split_once('=')
                .ok_or_else(|| Error::CliParse(format!("invalid input-file option: {option}")))?;
            task.options.push((key.trim().into(), value.trim().into()));
            continue;
        }

        let task = current.get_or_insert_with(|| InputTask {
            uris: Vec::new(),
            options: Vec::new(),
        });
        task.uris.push(line.trim().into());
    }

    flush_task(&mut tasks, &mut current);
    Ok(tasks)
}

pub fn save_session_text(tasks: &[InputTask]) -> String {
    let mut out = String::new();
    for (index, task) in tasks.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        for uri in &task.uris {
            out.push_str(uri);
            out.push('\n');
        }
        for (key, value) in &task.options {
            out.push_str("  ");
            out.push_str(key);
            out.push('=');
            out.push_str(value);
            out.push('\n');
        }
    }
    out
}

fn flush_task(tasks: &mut Vec<InputTask>, current: &mut Option<InputTask>) {
    if let Some(task) = current.take()
        && !task.uris.is_empty()
    {
        tasks.push(task);
    }
}

fn command() -> Command {
    Command::new("raria")
        .disable_help_flag(true)
        .arg(value_option("dir", "dir"))
        .arg(value_option("enable-rpc", "enable-rpc"))
        .arg(value_option("rpc-listen-port", "rpc-listen-port"))
        .arg(value_option("rpc-secret", "rpc-secret"))
        .arg(value_option("continue", "continue"))
        .arg(value_option("split", "split"))
        .arg(value_option("input-file", "input-file"))
        .arg(value_option("save-session", "save-session"))
        .arg(value_option(
            "enable-http-pipelining",
            "enable-http-pipelining",
        ))
        .arg(value_option("ed2k-server", "ed2k-server"))
        .arg(Arg::new("uris").action(ArgAction::Append).num_args(0..))
}

fn value_option(id: &'static str, long: &'static str) -> Arg {
    Arg::new(id)
        .long(long)
        .num_args(0..=1)
        .require_equals(false)
}

fn get_bool(matches: &clap::ArgMatches, name: &str) -> bool {
    matches
        .get_one::<String>(name)
        .map(|value| value != "false")
        .unwrap_or(false)
}

fn collect_dispositions(matches: &clap::ArgMatches) -> Vec<OptionDisposition> {
    let mut dispositions = Vec::new();
    if matches.contains_id("enable-http-pipelining") {
        dispositions.push(OptionDisposition::Pruned {
            name: "enable-http-pipelining".into(),
        });
    }
    if matches.contains_id("ed2k-server") {
        dispositions.push(OptionDisposition::UnsupportedPhaseOne {
            name: "ed2k-server".into(),
        });
    }
    dispositions
}
