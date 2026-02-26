use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

struct Weather {
    samples: u32,
    min: f64,
    mean: f64,
    max: f64,
}

fn main() {
    let filename = std::env::args().nth(1).expect("missing filename");
    let file = File::open(filename).expect("could not open file");
    let file = BufReader::new(file);
    let mut stats = HashMap::new();

    for line in file.lines() {
        let line = line.unwrap();
        if line.starts_with('#') {
            continue;
        }
        let (city, temperature) = line.split_once(';').unwrap();
        let temperature = temperature.parse::<f64>().unwrap();
        stats
            .entry(city.to_string())
            .and_modify(|entry: &mut Weather| {
                if temperature < entry.min {
                    entry.min = temperature;
                } else if temperature > entry.max {
                    entry.max = temperature;
                }

                entry.mean += temperature;
                entry.samples += 1;
            })
            .or_insert(Weather {
                samples: 1,
                min: temperature,
                mean: temperature,
                max: temperature,
            });
    }

    let mut stats: Vec<(String, f64, f64, f64)> = stats
        .into_iter()
        .map(|(city, weather)| {
            (
                city,
                weather.min,
                weather.mean / weather.samples as f64,
                weather.max,
            )
        })
        .collect();

    stats.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    print!("{{");
    let len = stats.len();
    for stat in stats.iter().take(len - 1) {
        print!("{}={:.1}/{:.1}/{:.1}, ", stat.0, stat.1, stat.2, stat.3);
    }
    println!(
        "{}={:.1}/{:.1}/{:.1}}}",
        stats[len - 1].0,
        stats[len - 1].1,
        stats[len - 1].2,
        stats[len - 1].3
    );
}
