use clap::Parser;
use indicatif::ParallelProgressIterator;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use rayon::prelude::*;
use regex::Regex;
use std::{
    fs::File,
    io::{Read, Write},
    process::{Command, Stdio},
};

/// A simple tester for the EDA Game
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    player1: String,
    player2: String,
    player3: String,
    player4: String,

    /// Number of instances to run
    #[arg(short, long, default_value_t = 100)]
    instances: u32,

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
    instances: u32,
    players: [PlayerName; 4],
    settings_file: String,
}

fn main() {
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

    run_tests(config);
}

fn run_tests(config: TestConfig) {
    let min_seed = config.seed;
    let Some(max_seed) = config.seed.checked_add(config.instances - 1)
    else {
        todo!("Deal with out of bounds ranges");
    };

    let re = Regex::new(r"player \S* got score (\d*)").unwrap();

    let mut f = File::open(config.settings_file).unwrap();
    let mut settings = String::new();
    f.read_to_string(&mut settings).unwrap();

    let pb = ProgressBar::new(config.instances.into()).with_style(
        ProgressStyle::with_template(" Running games... ({pos}/{len}) {wide_bar} {percent}% ")
            .unwrap(),
    );

    pb.tick();

    let results = (min_seed..=max_seed)
        .into_par_iter()
        .map(|seed| {
            let mut child = Command::new("./Game")
                .args(config.players.map(|p| p.as_string()))
                .arg("-s")
                .arg(seed.to_string())
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();

            let mut stdin = child.stdin.take().unwrap();
            stdin.write_all(settings.as_bytes()).unwrap();

            let mut stderr = child.stderr.take().unwrap();
            let mut output = String::new();
            stderr.read_to_string(&mut output).unwrap();

            if !child.wait().unwrap().success() {
                println!("Game crashed!");
            }

            let mut ret = [(0usize, 0usize); 4];

            for (i, points) in re
                .captures_iter(&output)
                .map(|caps| caps.get(1).unwrap().as_str().parse::<usize>().unwrap())
                .enumerate()
            {
                ret[i].0 = points;
            }
            let max_points = ret.iter().map(|(x, _)| x).max().unwrap().to_owned();

            for i in 0..4 {
                ret[i].1 = if ret[i].0 == max_points { 1 } else { 0 };
            }

            ret
        })
        .progress_with(pb)
        .reduce(
            || [(0usize, 0usize); 4],
            |a, b| {
                let mut ret = [(0usize, 0usize); 4];
                for i in 0..4 {
                    ret[i].0 = a[i].0 + b[i].0;
                    ret[i].1 = a[i].1 + b[i].1;
                }

                ret
            },
        );

    for (i, res) in results.iter().enumerate() {
        println!(
            "Player {} got {} points in average ({}% WR)",
            config.players[i].as_string(),
            res.0 as f32 / config.instances as f32,
            res.1 as f32 * 100. / config.instances as f32,
        );
    }
}
