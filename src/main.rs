#![warn(clippy::pedantic)]

mod errors;
use errors::AppError;

use clap::Parser;
use color_eyre::eyre::Result;
use indicatif::ParallelProgressIterator;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use rayon::prelude::*;
use regex::Regex;
use std::num::NonZeroU32;
use std::{
    fs::File,
    io::{Read, Write},
    process::{Command, Stdio},
};

/// A simple tester for the EDA Game
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Name of player 1
    player1: String,

    /// Name of player 2
    player2: String,

    /// Name of player 3
    player3: String,

    /// Name of player 4
    player4: String,

    /// Number of instances to run
    #[arg(short, long, default_value_t = NonZeroU32::new(100).unwrap())]
    instances: NonZeroU32,

    /// Initial seed to test
    #[arg(short, long, default_value_t = 0)]
    seed: u32,

    /// Game settings file
    #[arg(short, long, default_value_t = String::from("default.cnf"))]
    game_settings: String,
}

#[derive(Clone, Copy)]
struct PlayerName([u8; 12]);

impl PlayerName {
    fn as_string(&self) -> String {
        String::from_utf8_lossy(&self.0)
            .trim_end_matches('\0')
            .to_owned()
    }
}

impl TryFrom<&str> for PlayerName {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.len() > 12 {
            return Err(());
        }

        let mut ret = PlayerName([0; 12]);
        for (i, byte) in value.bytes().enumerate() {
            ret.0[i] = byte;
        }

        Ok(ret)
    }
}

struct TestConfig {
    seed: u32,
    instances: NonZeroU32,
    players: [PlayerName; 4],
    settings_file: String,
}

fn main() -> Result<()> {
    color_eyre::install()?;

    let args = Args::parse();

    let config = TestConfig {
        seed: args.seed,
        instances: args.instances,
        players: [
            args.player1.as_str().try_into().unwrap(),
            args.player2.as_str().try_into().unwrap(),
            args.player3.as_str().try_into().unwrap(),
            args.player4.as_str().try_into().unwrap(),
        ],
        settings_file: args.game_settings,
    };

    run_tests(config)?;

    Ok(())
}

enum ExecutionResults {
    Ok { points: [u32; 4] },
    Crash { seed: u32 },
}

impl Default for ExecutionResults {
    fn default() -> Self {
        Self::Ok { points: [0; 4] }
    }
}

#[derive(Default)]
struct PlayerResults {
    total_points: u32,
    total_wins: u32,
}

#[derive(Default)]
struct TestResults {
    player_results: [PlayerResults; 4],
    failed_seeds: Vec<u32>,
}

fn run_tests(config: TestConfig) -> Result<()> {
    let min_seed = config.seed;

    let max_seed = config
        .seed
        .checked_add(config.instances.get() - 1)
        .ok_or(AppError::SeedRangeOutOfBounds)?;

    let re = Regex::new(r"player \S* got score (\d*)")?;

    let mut f = File::open(config.settings_file)?;
    let mut settings = String::new();
    f.read_to_string(&mut settings)?;

    let pb = ProgressBar::new(config.instances.get().into()).with_style(ProgressStyle::with_template(
        " Running games... ({pos}/{len}) {wide_bar} {percent}% ",
    )?);

    pb.tick();

    let results = (min_seed..=max_seed)
        .into_par_iter()
        .map::<_, Result<_>>(|seed| {
            let mut child = Command::new("./Game")
                .args(config.players.map(|p| p.as_string()))
                .arg("-s")
                .arg(seed.to_string())
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()?;

            let mut stdin = child
                .stdin
                .take()
                .ok_or(AppError::BrokenChildCommunication)?;
            stdin.write_all(settings.as_bytes())?;

            let mut stderr = child
                .stderr
                .take()
                .ok_or(AppError::BrokenChildCommunication)?;
            let mut output = String::new();
            stderr.read_to_string(&mut output)?;

            if !child.wait()?.success() {
                return Ok(ExecutionResults::Crash { seed });
            }

            let mut ret = [0u32; 4];

            for (i, points) in re
                .captures_iter(&output)
                .map(|caps| caps.get(1).unwrap().as_str().parse().unwrap())
                .enumerate()
            {
                ret[i] = points;
            }

            Ok(ExecutionResults::Ok { points: ret })
        })
        .progress_with(pb)
        .map::<_, Result<_>>(|x| {
            let mut ret = TestResults::default();
            match x? {
                ExecutionResults::Ok { points } => {
                    for i in 0..4 {
                        ret.player_results[i].total_points = points[i];
                        if points[i] == *points.iter().max().unwrap() {
                            ret.player_results[i].total_wins = 1;
                        }
                    }
                }
                ExecutionResults::Crash { seed } => ret.failed_seeds = vec![seed],
            }
            Ok(ret)
        })
        .reduce(
            || Ok(TestResults::default()),
            |a, b| {
                let mut a = a?;
                let b = b?;

                a.failed_seeds.extend_from_slice(&b.failed_seeds);
                for i in 0..4 {
                    a.player_results[i].total_points += b.player_results[i].total_points;
                    a.player_results[i].total_wins += b.player_results[i].total_wins;
                }

                Ok(a)
            },
        )?;

    println!("Game results:");
    #[allow(clippy::cast_possible_truncation)] // Correctness: We can't run more than u32::MAX seeds
    let ok_games = config.instances.get() - results.failed_seeds.len() as u32;

    for (i, res) in results.player_results.iter().enumerate() {
        println!(
            "=> Player {} got {} points in average ({}% WR)",
            config.players[i].as_string(),
            f64::from(res.total_points) / f64::from(ok_games),
            f64::from(res.total_wins) * 100. / f64::from(ok_games),
        );
    }
    println!();

    if !results.failed_seeds.is_empty() {
        println!("Some games crashed! Faulty seeds:");
        for seed in results.failed_seeds {
            println!("=> {seed}");
        }
    }

    Ok(())
}
