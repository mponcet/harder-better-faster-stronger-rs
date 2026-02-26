use libc::{c_int, c_void, memchr};
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
    let mut file = BufReader::new(file);
    let mut stats: HashMap<String, Weather> = HashMap::new();

    let mut buf = Vec::with_capacity(100);
    while let Ok(n) = file.read_until(b'\n', &mut buf)
        && n > 0
    {
        let line = unsafe { str::from_utf8_unchecked(&buf[..n - 1]) };
        if line.starts_with('#') {
            continue;
        }

        let pos =
            unsafe { memchr(line.as_ptr() as *const c_void, b';' as c_int, n - 1) } as *const u8;
        let pos = unsafe { pos.offset_from(line.as_ptr()) } as usize;
        let (city, temperature) = (&line[..pos], &line[pos + 1..line.len()]);
        let temperature = temperature.parse::<f64>().unwrap();

        if let Some(entry) = stats.get_mut(city) {
            if temperature < entry.min {
                entry.min = temperature;
            } else if temperature > entry.max {
                entry.max = temperature;
            }

            entry.mean += temperature;
            entry.samples += 1;
        } else {
            stats.insert(
                city.to_string(),
                Weather {
                    samples: 1,
                    min: temperature,
                    mean: temperature,
                    max: temperature,
                },
            );
        }

        buf.clear();
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
