use libc::{c_int, c_void, memchr};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};

struct Weather {
    samples: u32,
    min: i16,
    mean: i64,
    max: i16,
}

#[inline]
fn parse_temperature(mut temperature: &[u8]) -> i16 {
    let to_digit = |b: u8| -> i16 { (b - b'0') as i16 };
    let is_negative = temperature[0] == b'-';
    if is_negative {
        temperature = &temperature[1..];
    }

    // Single digit temperature.
    let result = if temperature[1] == b'.' {
        to_digit(temperature[0]) * 10 + to_digit(temperature[2])
    } else {
        to_digit(temperature[0]) * 100 + to_digit(temperature[1]) * 10 + to_digit(temperature[3])
    };

    if is_negative { -result } else { result }
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
        if buf[0] == b'#' {
            continue;
        }
        let line = unsafe { str::from_utf8_unchecked(&buf[..n - 1]) };

        let pos =
            unsafe { memchr(line.as_ptr() as *const c_void, b';' as c_int, n - 1) } as *const u8;
        let pos = unsafe { pos.offset_from(line.as_ptr()) } as usize;
        let (city, temperature) = (&line[..pos], &line[pos + 1..line.len()]);
        let temperature = parse_temperature(temperature.as_bytes());

        if let Some(entry) = stats.get_mut(city) {
            if temperature < entry.min {
                entry.min = temperature;
            } else if temperature > entry.max {
                entry.max = temperature;
            }

            entry.mean += temperature as i64;
            entry.samples += 1;
        } else {
            stats.insert(
                city.to_string(),
                Weather {
                    samples: 1,
                    min: temperature,
                    mean: temperature as i64,
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
                weather.min as f64 / 10.0,
                (weather.mean as f64 / 10.0 / weather.samples as f64),
                weather.max as f64 / 10.0,
            )
        })
        .collect();

    stats.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    print!("{{");
    let len = stats.len();
    for stat in stats.iter().take(len - 1) {
        print!("{}={:.1}/{:.1}/{:.1}, ", stat.0, stat.1, stat.2, stat.3,);
    }
    println!(
        "{}={:.1}/{:.1}/{:.1}}}",
        stats[len - 1].0,
        stats[len - 1].1,
        stats[len - 1].2,
        stats[len - 1].3
    );
}
